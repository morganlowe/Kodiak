# Demo Hardware Project

A sample hardware documentation page illustrating schematics, pinouts, components, and project notes.

## Overview

This is a demonstration project template rendered by Kodiak ProjectsWiki. When you run `wiki sync` or use the Kodiak Setup Wizard, this catalog automatically indexes your local project folders and synthesizes documentation using your local AI model.

## Specifications

- **Microcontroller:** ESP32-S3-WROOM-1
- **Clock Speed:** 240 MHz Dual Core
- **Connectivity:** Wi-Fi 802.11 b/g/n, Bluetooth 5 (LE)
- **Power Input:** 5V USB-C or 3.7V LiPo

## Pinout & Wiring

| Pin | Function | Description |
| :--- | :--- | :--- |
| GPIO 4 | I2C SDA | Data line for external sensors |
| GPIO 5 | I2C SCL | Clock line for external sensors |
| GPIO 18 | SPI SCK | SPI Clock |
| GPIO 19 | SPI MISO | SPI Master In / Slave Out |
| GPIO 23 | SPI MOSI | SPI Master Out / Slave In |

## Getting Started

1. Place your project code or hardware repository into your configured projects directory.
2. Click **Sync** in the Explorer or run `wiki sync` in your terminal.
3. Open the project in Kodiak to inspect, build documentation, or review schematics.
