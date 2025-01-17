import socket

server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
server_socket.bind(('0.0.0.0', 12483))
server_socket.listen(5)
print("Server started, waiting for connections...")

while True:
    client_socket, addr = server_socket.accept()
    print(f"Connection from {addr}")
    while True:
        data = client_socket.recv(1024)
        if not data:
            break
        print(f"Received: {data.decode()}")
    client_socket.close()
    print(f"Connection from {addr} closed")
