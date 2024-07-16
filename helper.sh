#!/bin/bash

# Define the array of bytes
bytes=(118 110 101 116 45 57 57 48 70 49 69)

# Convert each byte to its ASCII character and concatenate into a string
string=""
for byte in "${bytes[@]}"; do
    string+="$(printf "\\$(printf '%03o' "$byte")")"
done

# Print the resulting string
echo "$string"
