use anyhow::Result;
use attenuation::DB_11;
use esp_idf_hal::adc::*;
use esp_idf_hal::adc::oneshot::*;
use esp_idf_hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::gpio::{Gpio2, Gpio34, Gpio4, PinDriver};
use esp_idf_hal::modem::Modem;
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::log::*;
use esp_idf_svc::nvs::*;
use esp_idf_svc::wifi::*;
use esp_idf_sys::TickType_t;
use esp_idf_sys::xTaskCreatePinnedToCore;
use esp_idf_sys::{ xQueueGenericSend, xQueueReceive, QueueHandle_t};
use esp_idf_sys::{self as _, uxQueueMessagesWaiting, xQueueGenericCreate};
use heapless::String as HeaplessString;
use log::*;
use rand::Rng;
use std::io::Write;
use std::net::TcpStream;
use std::ptr;

const SSID: &str = "***";
const PASSWORD: &str = "***";
const SERVER_IP: &str = "***"; 
const SERVER_PORT: u16 = 12483;
const QUEUE_LENGTH: u32 = 10;
const QUEUE_ITEM_SIZE: u32 = std::mem::size_of::<u16>() as u32;
const QUEUE_WAIT_TIME: TickType_t = 0;
static mut ADC_QUEUE: Option<QueueHandle_t> = None;

struct AdcTaskParams {
    pin: Gpio34,
    adc: ADC1,
}

struct BlinkTaskParams {
    pin_red: Gpio4,
    pin_green: Gpio2
}

unsafe extern "C" fn adc_task(params: *mut core::ffi::c_void) {
    let params = params as *mut AdcTaskParams;
    let adc = AdcDriver::new((*params).adc.clone_unchecked()).unwrap();
    let config = AdcChannelConfig {
        attenuation: DB_11,
        calibration: true,
        ..Default::default()
    };
    let mut adc_pin = AdcChannelDriver::new(&adc, (*params).pin.clone_unchecked(), &config).unwrap();
    
    loop {
        let value = adc.read(&mut adc_pin).unwrap();
        xQueueGenericSend(ADC_QUEUE.unwrap(), &value as *const u16 as *const core::ffi::c_void, QUEUE_WAIT_TIME, 0);
        println!("ADC value: {} ", value);
        FreeRtos::delay_ms(1000);
    }
}

unsafe extern "C" fn tcp_task(ptr: *mut core::ffi::c_void) {
    loop {
        let mut adc_value: u16 = 0;
        if uxQueueMessagesWaiting(ADC_QUEUE.unwrap()) > 0 && xQueueReceive(ADC_QUEUE.unwrap(), &mut adc_value as *mut u16 as *mut core::ffi::c_void, 1000) == 1 {
            let payload = adc_value.to_string();
            let mut stream = TcpStream::connect((SERVER_IP, SERVER_PORT)).unwrap();
            stream.write_all(payload.as_bytes()).unwrap();
            println!("Sent {} via TCP", payload);
        }
        FreeRtos::delay_ms(1000);
    }
}

unsafe extern "C" fn blink_task(params: *mut core::ffi::c_void) {
    let params: *mut BlinkTaskParams = params as *mut BlinkTaskParams;
    let mut led_green = PinDriver::output((*params).pin_green.clone_unchecked()).unwrap();
    let mut led_red = PinDriver::output((*params).pin_red.clone_unchecked()).unwrap();
    let mut rng = rand::thread_rng();

    loop {
        let delay_green: u32 = rng.gen_range(500..2000);
        let delay_red: u32 = rng.gen_range(500..2000);

        led_green.set_high().unwrap();
        FreeRtos::delay_ms(delay_green);
        led_green.set_low().unwrap();
        led_red.set_high().unwrap();
        FreeRtos::delay_ms(delay_red);
        led_red.set_low().unwrap();
    }
}

fn connect_wifi(modem: &mut Modem) -> Result<(EspWifi)> {
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();
    let mut wifi = EspWifi::new(
        modem,
        sys_loop,
        Some(nvs)
    )?;

    let ssid_vec = heapless::Vec::<u8, 32>::from_slice(SSID.as_bytes()).unwrap();
    let password_vec = heapless::Vec::<u8, 64>::from_slice(PASSWORD.as_bytes()).unwrap();
    
    let ssid: HeaplessString<32> = HeaplessString::from_utf8(ssid_vec).unwrap();
    let password: HeaplessString<64> = HeaplessString::from_utf8(password_vec).unwrap();
    wifi.set_configuration(&Configuration::Client(ClientConfiguration{
        ssid,
        password,
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        channel: None,
        scan_method: ScanMethod::default(),
        pmf_cfg: PmfConfiguration::default()
    })).unwrap();

    wifi.start()?;
    wifi.connect()?;

    info!("Connecting to Wi-Fi...");
    while !wifi.is_connected()? {
        FreeRtos::delay_ms(1000);
        println!("Waiting for Wi-Fi...");
    }
    info!("Connected to Wi-Fi");
    Ok(wifi)
}

fn main() -> Result<()> {
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let adc1 = peripherals.adc1;
    let mut modem = peripherals.modem;
    let gpio34 = peripherals.pins.gpio34;
    let gpio2 = peripherals.pins.gpio2;
    let gpio4 = peripherals.pins.gpio4;
    let bul = connect_wifi(&mut modem).unwrap();

    unsafe {
        let queue = xQueueGenericCreate(QUEUE_LENGTH, QUEUE_ITEM_SIZE, 0);
        while queue.is_null() {
            println!("Failed to create queue");
            FreeRtos::delay_ms(1000);
        }
        ADC_QUEUE = Some(queue);
    }

    let adc_params = AdcTaskParams {
        adc: adc1,
        pin: gpio34,
    };

    let blink_params = BlinkTaskParams {
        pin_green: gpio2,
        pin_red: gpio4,
    };

    FreeRtos::delay_ms(5000);
    unsafe {
        xTaskCreatePinnedToCore(
            Some(adc_task),
            b"ADC Task\0".as_ptr() as *const core::ffi::c_char,
            4096,
            &adc_params as *const _ as *mut core::ffi::c_void,
            esp_idf_sys::ESP_TASK_PRIO_MIN,
            ptr::null_mut(),
            0
        );

        xTaskCreatePinnedToCore(
            Some(tcp_task),
            b"TCP Task\0".as_ptr() as *const core::ffi::c_char,
            4096,

            ptr::null_mut(),
            esp_idf_sys::ESP_TASK_PRIO_MIN,
            ptr::null_mut(),
            0
        );

        xTaskCreatePinnedToCore(
            Some(blink_task),
            b"Blink Task\0".as_ptr() as *const core::ffi::c_char,
            4096,
            &blink_params as *const _ as *mut core::ffi::c_void,
            esp_idf_sys::ESP_TASK_PRIO_MIN,
            ptr::null_mut(),
            0
        );
    }

    loop {
        FreeRtos::delay_ms(500);
    }
}
