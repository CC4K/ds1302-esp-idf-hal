use std::f32;
use std::thread;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use std::sync::{mpsc, Mutex, Arc};
use std::sync::mpsc::channel;
use std::ffi::CString;
use esp_idf_svc::log::EspLogger;
use esp_idf_sys::esp_log_level_set;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::gpio::*;
use esp_idf_hal::delay::{Ets, Delay};
use esp_idf_hal::task::thread::ThreadSpawnConfiguration;
use dht11::Dht11;


enum ButtonEvent {
    ShortPress,
    LongPress,
    DoublePress
}

enum RTCEvent {
    Tick(u8, u8, u8) // (hour, minute, second)
}

enum SensorData {
    Temperature(f32),
    Moisture(f32),
    Light(bool),
    Pressure(f32)
}

#[derive(PartialEq)]
enum SetupMode {
    Idle,
    Hours,
    Minutes,
    Seconds,
}

/// Measure for how long the button is pressed in ms
fn measure_press_duration(btn: &PinDriver<'static, AnyIOPin, Input>) -> u32 {
    let start = Instant::now();
    while btn.is_low() {}
    start.elapsed().as_millis() as u32
}

/// Check for a second short press after a first short press for a certain amount of time
fn detect_double_press(btn: &PinDriver<'static, AnyIOPin, Input>, wait_ms: u32) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(wait_ms as u64) {
        if btn.is_low() {
            while btn.is_low() {}
            return true;
        }
    }
    false
}

/// Determine the type of button press
fn determine_press_type(btn: &PinDriver<'static, AnyIOPin, Input>) -> ButtonEvent {
    let duration = measure_press_duration(btn);
    if duration >= 2000 {
        // log::info!("Long press detected (pressed for {duration}ms)");
        ButtonEvent::LongPress
    }
    else if duration > 0 && duration < 2000 {
        if detect_double_press(btn, 300) {
            // log::info!("Double press detected");
            ButtonEvent::DoublePress
        }
        else {
            // log::info!("Short press detected");
            ButtonEvent::ShortPress
        }
    }
    else {
        // log::info!("Short press detected");
        ButtonEvent::ShortPress
    }
}

/// Calculate atmospheric pressure from temperature and humidity
fn get_atm_pressure(t_c: f32, hour: f32) -> f32 {
    let t0 = t_c + 273.15;
    let rh = hour/100.0;
    let p0 = 1013.25;
    let z = 100.0;
    let g = 9.8066;
    let r = 8.3145;
    let minute = 0.0289;
    let l = 0.0065;
    let e_s = 6.112 * f32::powf(f32::consts::E, (17.67 * t_c) / (t_c + 243.5));
    let e = (rh * e_s) / 100.0;
    let press = p0 * f32::powf(1.0 - (l * z) / t0, (g * minute) / (r * l));
    let p_dry = press - e;
    p_dry
}

/// Convert Binary Coded Decimal to Decimal
fn bcd_to_dec(bcd: u8) -> u8 {
    ((bcd >> 4)*10) + (bcd & 0x0F)
}
/// Convert Decimal to Binary Coded Decimal
fn dec_to_bcd(dec: u8) -> u8 {
    ((dec/10) << 4) | (dec%10)
}

struct RTCInterface {
    sclk: PinDriver<'static, Gpio1, Output>,
    io: Option<PinDriver<'static, Gpio2, Output>>,
    ce: PinDriver<'static, Gpio3, Output>,
    delay: Delay,
    hours: u8,
    minutes: u8,
    seconds: u8,
}

impl RTCInterface {
    /// Initialize RTC
    #[allow(unused_mut)]
    fn new(mut sclk: PinDriver<'static, Gpio1, Output>, mut io: Option<PinDriver<'static, Gpio2, Output>>, mut ce: PinDriver<'static, Gpio3, Output>, delay: Delay) -> Self {
        let hours = dec_to_bcd(14);
        let minutes = dec_to_bcd(50);
        let seconds = dec_to_bcd(00);

        // /!\ You can also set the clock directly to an initial time by uncommenting the following comment block (comment it back or it will reset on every run) /!\ //
        /*

        let io_pin = io.as_mut().unwrap();
        // DISABLE WRITE PROTECTION //
        ce.set_low().unwrap();        
        // start transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x8E on control register using IO pin
        io_pin.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x8E >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        // send data 0x00 to disable write protection
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x00 >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        // end transaction
        ce.set_low().unwrap();

        // SET SECONDS (CLEAR CH BIT) //
        // start transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x80 on control register using IO pin
        io_pin.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x80 >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        // send data seconds and clear bit 7
        let sec = seconds & 0x7F;
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (sec >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        // end transaction
        ce.set_low().unwrap();

        // SET MINUTES //
        // start transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x82 on control register using IO pin
        io_pin.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x82 >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        // send data minutes
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (minutes >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        ce.set_low().unwrap();

        // SET HOURS (24H MODE) //
        // begin transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x84 on control register using IO pin
        io_pin.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x84 >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        // send data hours and set bit 7 to 0 for 24h mode
        let hrs = hours & 0x3F;
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (hrs >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            delay.delay_us(1);
            sclk.set_high().unwrap();
            delay.delay_us(1);
        }
        // end transaction
        ce.set_low().unwrap();

        */

        // SET FIELDS //
        RTCInterface { sclk, io, ce, delay, hours, minutes, seconds }
    }

    /// Iterate seconds register
    fn iterate_second(&mut self) {
        self.read();
        let io_pin = self.io.as_mut().unwrap();
        self.ce.set_low().unwrap();
        // start transaction
        self.sclk.set_low().unwrap();
        self.ce.set_high().unwrap();
        // write byte 0x80 on control register using IO pin
        io_pin.set_low().unwrap();
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            if (0x80 >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            self.delay.delay_us(1);
            self.sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        if self.seconds == 59 { self.seconds = dec_to_bcd(0); } else { self.seconds = dec_to_bcd(self.seconds+1); }
        // send data seconds and clear bit 7
        let sec_val = self.seconds & 0x7F;
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            if (sec_val >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            self.delay.delay_us(1);
            self.sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // end transaction
        self.ce.set_low().unwrap();
    }

    /// Iterate minutes register
    fn iterate_minute(&mut self) {
        self.read();
        let io_pin = self.io.as_mut().unwrap();
        self.ce.set_low().unwrap();
        // start transaction
        self.sclk.set_low().unwrap();
        self.ce.set_high().unwrap();
        // write byte 0x82 on control register using IO pin
        io_pin.set_low().unwrap();
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            if (0x82 >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            self.delay.delay_us(1);
            self.sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        if self.minutes == 59 { self.minutes = dec_to_bcd(0); } else { self.minutes = dec_to_bcd(self.minutes+1); }
        // send data minutes
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            if (self.minutes >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            self.delay.delay_us(1);
            self.sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // end transaction
        self.ce.set_low().unwrap();
    }

    /// Iterate hours register
    fn iterate_hour(&mut self) {
        self.read();
        let io_pin = self.io.as_mut().unwrap();
        self.ce.set_low().unwrap();
        // start transaction
        self.sclk.set_low().unwrap();
        self.ce.set_high().unwrap();
        // write byte 0x84 on control register using IO pin
        io_pin.set_low().unwrap();
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            if (0x84 >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            self.delay.delay_us(1);
            self.sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        if self.hours == 23 { self.hours = dec_to_bcd(0); } else { self.hours = dec_to_bcd(self.hours+1); }
        // send data hours and set bit 7 to 0 for 24h mode
        let hrs = self.hours & 0x3F;
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            if (hrs >> i) & 0x01 == 1 { io_pin.set_high().unwrap(); } else { io_pin.set_low().unwrap(); }
            self.delay.delay_us(1);
            self.sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // end transaction
        self.ce.set_low().unwrap();
    }

    /// Read the clock updates the time fields
    fn read(&mut self) {
        self.io.as_mut().unwrap().set_low().unwrap();
        // begin transaction
        self.sclk.set_low().unwrap();
        self.ce.set_high().unwrap();
        self.delay.delay_us(1);
        // send burst read command 0xBF
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            if (0xBF >> i) & 0x01 == 1 { self.io.as_mut().unwrap().set_high().unwrap(); }
            else { self.io.as_mut().unwrap().set_low().unwrap(); }
            self.delay.delay_us(1);
            self.sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // set io_pin to input mode
        let input_pin = self.io.take().unwrap().into_input().unwrap();
        // read seconds, minutes, and hours
        let mut sec_val = 0;
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            let bit = if input_pin.is_high() { 1 } else { 0 };
            sec_val |= bit << i;
            self.sclk.set_high().unwrap();
        }
        let mut min_val = 0;
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            let bit = if input_pin.is_high() { 1 } else { 0 };
            min_val |= bit << i;
            self.sclk.set_high().unwrap();
        }
        let mut hr_val = 0;
        for i in 0..8 {
            self.sclk.set_low().unwrap();
            let bit = if input_pin.is_high() { 1 } else { 0 };
            hr_val |= bit << i;
            self.sclk.set_high().unwrap();
        }
        // skip unused registers (date, month, year, etc)
        for _ in 0..(8 * 4) {
            self.sclk.set_low().unwrap();
            let _ = if input_pin.is_high() { 1 } else { 0 };
            self.sclk.set_high().unwrap();
        }
        // end transaction
        self.ce.set_low().unwrap();
        // set io_pin back to output
        let io_pin = input_pin.into_output().unwrap();
        self.io = Some(io_pin);
        // translate values and update fields
        sec_val = bcd_to_dec(sec_val & 0x7F);
        min_val = bcd_to_dec(min_val & 0x7F);
        hr_val = bcd_to_dec(hr_val & 0x3F);
        self.seconds = sec_val;
        self.minutes = min_val;
        self.hours = hr_val;
    }
}


/// Send ButtonEvents
fn button_task(tx_button: mpsc::Sender<ButtonEvent>, btn_pin: PinDriver<'static, AnyIOPin, Input>) {
    loop {
        if btn_pin.is_low() {
            let press_event = determine_press_type(&btn_pin);
            tx_button.send(press_event).unwrap();
            std::thread::sleep(Duration::from_millis(300)); // stop listening for 300ms
        }
        std::thread::sleep(Duration::from_millis(100)); // check every 100ms to be reactive
    }
}

/// Read the RTC every second and send event to the display task
fn rtc_task(tx_rtc: mpsc::Sender<RTCEvent>, rtc_mutex: Arc<Mutex<RTCInterface>>) {
    loop {
        let mut rtc = rtc_mutex.lock().unwrap();
        rtc.read();
        let hour = rtc.hours;
        let minute = rtc.minutes;
        let second = rtc.seconds;
        tx_rtc.send(RTCEvent::Tick(hour, minute, second)).unwrap();
        drop(rtc); // drop the lock
        thread::sleep(Duration::from_millis(1000));
    }
}

/// Read the light level, temperature and moisture, calculate the pressure and send events to the display task
fn sensor_task(tx_sensor: mpsc::Sender<SensorData>, mut dht11: Dht11<PinDriver<'static, AnyIOPin, InputOutput>>, light_pin: PinDriver<'static, Gpio38, Input>) {
    loop {
        if let Ok(measurement) = dht11.perform_measurement(&mut Ets) {
            let temperature = measurement.temperature as f32 /10.0;
            let humidity = measurement.humidity as f32 /10.0;
            let pressure = get_atm_pressure(temperature, humidity);
            tx_sensor.send(SensorData::Temperature(temperature)).unwrap();
            tx_sensor.send(SensorData::Moisture(humidity)).unwrap();
            tx_sensor.send(SensorData::Pressure(pressure)).unwrap();
        }
        tx_sensor.send(SensorData::Light(light_pin.is_low())).unwrap();
        thread::sleep(Duration::from_millis(200));
    }
}

/*
// HC-SR04 ultrasonic distance sensor (no crate)
fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let delay = Delay::default();

    // setup HC-SR04 sensor pins
    let mut trig_pin = PinDriver::output(peripherals.pins.gpio19).unwrap();
    let mut echo_pin = PinDriver::input(peripherals.pins.gpio20).unwrap();
    echo_pin.set_interrupt_type(InterruptType::AnyEdge).unwrap();

    loop {
        trig_pin.set_low().unwrap();
        delay.delay_ms(2);
        // trigger pulse
        trig_pin.set_high().unwrap();
        delay.delay_us(10);
        trig_pin.set_low().unwrap();
        // wait-for-rising
        while echo_pin.is_low() {
            if Instant::now() > Instant::now() + Duration::from_millis(50) {
                log::info!("Timeout"); 
                continue;
            }
        }
        let start = Instant::now();
        // wait-for-falling
        while echo_pin.is_high() {
            if Instant::now() > Instant::now() + Duration::from_millis(50) {
                log::info!("Timeout");
                continue;
            }
        }
        let end = Instant::now();
        let duration = (end - start).as_micros() as f32;
        if duration >= 38000. { log::info!("Out of range"); }

        let sound_speed = 0.03434; // at 20°C
        // let temp = 20.3; let sound_speed = 331.3 + (0.606 * temp); // alternative calculation
        let distance = (sound_speed * duration)/2.;
        log::info!("Distance: {:.2}cm", distance);

        delay.delay_ms(1000);
    }

}
 */

/*
// BMP180 digital pressure sensor
use bmp180_driver::{Common, Resolution, BMP180};
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::i2c::config::Config;

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let delay = Delay::default();

    // setup BMP180 sensor pins
    let scl_pin = peripherals.pins.gpio19;
    let sda_pin = peripherals.pins.gpio20;

    // initialize I2C driver
    let i2c = I2cDriver::new(peripherals.i2c0, sda_pin, scl_pin, &Config::default()).unwrap();

    // create BMP180 sensor instance
    let mut sensor = BMP180::new(i2c, delay);
    // check connection to the sensor
    sensor.check_connection().unwrap();
    // initialize sensor
    let mut sensor = sensor.initialize().unwrap();

    loop {
        let (temperature, pressure, altitude) = sensor.read_all(Resolution::UltraHighResolution).unwrap();
        log::info!("------------------------------------------------");
        log::info!("Temperature: {:.2}°C", temperature);
        log::info!("Pressure: {:.2}hPa", pressure/100);
        log::info!("Altitude: {:.2}m", altitude);

        delay.delay_ms(1000);
    }

}
 */

/*
// Tilt sensor
fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let delay = Delay::default();

    // setup tilt sensor pins
    let tilt_pin = PinDriver::input(peripherals.pins.gpio38).unwrap();

    loop {
        if tilt_pin.is_low() { log::info!("Straight up"); } else { log::info!("Tilted"); }

        delay.delay_ms(1000);
    }

}
*/

/*
// Vibration sensor
fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let delay = Delay::default();

    // setup vibration sensor
    let vibration_pin = PinDriver::input(peripherals.pins.gpio38).unwrap();

    loop {
        if vibration_pin.is_low() { log::info!("Still"); } else { log::info!("Vibrating"); }

        delay.delay_ms(1000);
    }

}
*/

/// Listen on the sensors, button, and RTC channels and display all the data
fn display_task(rx_sensor: mpsc::Receiver<SensorData>, rx_button: mpsc::Receiver<ButtonEvent>, rx_rtc: mpsc::Receiver<RTCEvent>,
    mut red_pin: PinDriver<'static, Gpio5, Output>, mut yellow_pin: PinDriver<'static, Gpio6, Output>, mut green_pin: PinDriver<'static, Gpio7, Output>, rtc_mutex: Arc<Mutex<RTCInterface>>)
{
    let mut full_access_mode = false;
    let mut can_be_unlocked = false;
    let mut setup_mode = SetupMode::Idle;

    // store data in a hashmap
    let mut latest_data: HashMap<&'static str, SensorData> = HashMap::new();
    let sensor_order = ["temperature", "moisture", "light", "pressure"];
    let mut current_sensor_i: usize = 0;

    loop {
        // receive from RTC
        while let Ok(rtc_event) = rx_rtc.try_recv() {
            let RTCEvent::Tick(hour, minute, second) = rtc_event;
            log::info!("==================== Time: {:02}:{:02}:{:02} ====================", hour, minute, second);
            can_be_unlocked = minute % 2 == 1;
        }
        // receive from button
        while let Ok(button_event) = rx_button.try_recv() {
            match button_event {
                // for navigating the sensors data or switching/incrementing time fields
                ButtonEvent::ShortPress => {
                    if setup_mode != SetupMode::Idle {
                        // if setup mode => ShortPress increments the current time field
                        let mut rtc = rtc_mutex.lock().unwrap();
                        match setup_mode {
                            SetupMode::Hours => { rtc.iterate_hour(); log::info!("[!] Incremented Hours"); },
                            SetupMode::Minutes => { rtc.iterate_minute(); log::info!("[!] Incremented Minutes"); },
                            SetupMode::Seconds => { rtc.iterate_second(); log::info!("[!] Incremented Seconds"); },
                            _ => {}
                        }
                    }
                    else {
                        // when no setup mode => navigate sensor data normally
                        current_sensor_i = (current_sensor_i+1)%sensor_order.len();
                    }
                },
                // for entering/exiting setup mode and looping between time fields
                ButtonEvent::LongPress => {
                    setup_mode = match setup_mode {
                        SetupMode::Idle => { log::info!("[!] Entering Setup Mode..."); SetupMode::Hours },
                        SetupMode::Hours => SetupMode::Minutes,
                        SetupMode::Minutes => SetupMode::Seconds,
                        SetupMode::Seconds => { log::info!("[!] Exiting Setup Mode..."); SetupMode::Idle },
                    }
                },
                // for entering/exiting full access mode
                ButtonEvent::DoublePress => {
                    if can_be_unlocked & !full_access_mode {
                        log::info!("[!] Entering Full Access Mode...");
                        full_access_mode = true;
                    }
                    else if full_access_mode {
                        log::info!("[!] Exiting Full Access Mode...");
                        full_access_mode = false;
                    }
                    else if !can_be_unlocked & !full_access_mode {
                        log::info!("[!] Cannot unlock Full Access Mode: minute is even");
                    }
                }
            }
        }
        // set LEDs according to current state of accessibility
        if full_access_mode {
            red_pin.set_low().unwrap();
            yellow_pin.set_low().unwrap();
            green_pin.set_high().unwrap();
        }
        else if !full_access_mode && can_be_unlocked{
            red_pin.set_high().unwrap();
            yellow_pin.set_high().unwrap();
            green_pin.set_low().unwrap();
        }
        else {
            red_pin.set_high().unwrap();
            yellow_pin.set_low().unwrap();
            green_pin.set_low().unwrap();
        }
        // receive from sensors
        while let Ok(sensor) = rx_sensor.try_recv() {
            match sensor {
                SensorData::Temperature(_) => latest_data.insert("temperature", sensor),
                SensorData::Moisture(_) => latest_data.insert("moisture", sensor),
                SensorData::Light(_) => latest_data.insert("light", sensor),
                SensorData::Pressure(_) => latest_data.insert("pressure", sensor)
            };
        }
        // display sensor data
        if full_access_mode & (setup_mode == SetupMode::Idle) {
            let sensor_name = sensor_order[current_sensor_i];
            match latest_data.get(sensor_name) {
                Some(SensorData::Temperature(temperature)) => log::info!("[FULL ACCESS MODE] Temperature: {temperature}°C"),
                Some(SensorData::Moisture(hour)) => log::info!("[FULL ACCESS MODE] Moisture: {hour}%"),
                Some(SensorData::Light(brightness)) => log::info!("[FULL ACCESS MODE] Light: {}", if *brightness { "Bright" } else { "Dark" }),
                Some(SensorData::Pressure(pressure)) => log::info!("[FULL ACCESS MODE] Pressure: {pressure}hPa"),
                _ => log::info!("[FULL ACCESS MODE] {sensor_name}: N/A")
            }
        }
        else if !full_access_mode & (setup_mode == SetupMode::Idle) {
            if let Some(SensorData::Temperature(temperature)) = latest_data.get("temperature") {
                log::info!("[RESTRICTED MODE] Temperature: {temperature}°C");
            }
            else {
                log::info!("[RESTRICTED MODE] Temperature: N/A");
            }
        }
        else if setup_mode != SetupMode::Idle {
            match setup_mode {
                SetupMode::Hours => log::info!("[RTC SETUP MODE] Selected: Hours"),
                SetupMode::Minutes => log::info!("[RTC SETUP MODE] Selected: Minutes"),
                SetupMode::Seconds => log::info!("[RTC SETUP MODE] Selected: Seconds"),
                SetupMode::Idle => {}
            }
        }

        thread::sleep(Duration::from_millis(1000));
    }
}




fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    // removing "gpio" info from logs
    let gpio_tag = CString::new("gpio").unwrap();
    unsafe { esp_log_level_set(gpio_tag.as_ptr(), esp_idf_sys::esp_log_level_t_ESP_LOG_NONE); }
    let peripherals = Peripherals::take().unwrap();
    let delay = Delay::default();

    // setup LEDs
    let red_pin = PinDriver::output(peripherals.pins.gpio5).unwrap();
    let yellow_pin = PinDriver::output(peripherals.pins.gpio6).unwrap();
    let green_pin = PinDriver::output(peripherals.pins.gpio7).unwrap();
    // setup button
    let mut btn_pin = PinDriver::input(peripherals.pins.gpio0.downgrade()).unwrap();
    btn_pin.set_pull(Pull::Up).unwrap();
    // setup DHT11 sensor
    let dht11_pin = PinDriver::input_output_od(peripherals.pins.gpio4.downgrade()).unwrap();
    let dht11 = Dht11::new(dht11_pin);
    // setup light sensor
    let light_pin = PinDriver::input(peripherals.pins.gpio38).unwrap();
    // setup RTC pins
    let sclk_pin = PinDriver::output(peripherals.pins.gpio1).unwrap();
    let io_dat_pin = Some(PinDriver::output(peripherals.pins.gpio2).unwrap());
    let ce_pin = PinDriver::output(peripherals.pins.gpio3).unwrap();
    let rtc = RTCInterface::new(sclk_pin, io_dat_pin, ce_pin, delay);
    let rtc_mutex = Arc::new(Mutex::new(rtc)); // needs to be shared between threads


    // create channels
    let (tx_rtc, rx_rtc) = channel();
    let (tx_button, rx_button) = channel();
    let (tx_sensor, rx_sensor) = channel();


    // spawn tasks
    /* RTC thread: CPU0 */
    ThreadSpawnConfiguration {
        name: Some("RTC\0".as_bytes()),
        pin_to_core: Some(esp_idf_hal::cpu::Core::Core0),
        ..Default::default()
    }.set().unwrap();

    let rtc_clone = Arc::clone(&rtc_mutex);
    thread::spawn(move || {
        rtc_task(tx_rtc, rtc_clone);
    });
    log::info!("RTC thread spawned");


    /* Button thread: CPU1 */
    ThreadSpawnConfiguration {
        name: Some("Button\0".as_bytes()),
        pin_to_core: Some(esp_idf_hal::cpu::Core::Core1),
        ..Default::default()
    }.set().unwrap();

    thread::spawn(move || { button_task(tx_button, btn_pin); });
    log::info!("Button thread spawned");


    /* Sensors thread: CPU1 */
    ThreadSpawnConfiguration {
        name: Some("Sensors\0".as_bytes()),
        pin_to_core: Some(esp_idf_hal::cpu::Core::Core1),
        ..Default::default()
    }.set().unwrap();

    thread::spawn(move || { sensor_task(tx_sensor, dht11, light_pin); });
    log::info!("Sensors thread spawned");


    /* Display thread: CPU1 */
    ThreadSpawnConfiguration {
        name: Some("Display\0".as_bytes()),
        pin_to_core: Some(esp_idf_hal::cpu::Core::Core1),
        ..Default::default()
    }.set().unwrap();

    let rtc_clone = Arc::clone(&rtc_mutex);
    thread::spawn(move || { display_task(rx_sensor, rx_button, rx_rtc, red_pin, yellow_pin, green_pin, rtc_clone); });
    log::info!("Display thread spawned");


    log::info!("Main thread finished")
}
