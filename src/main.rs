use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::gpio::*;
use esp_idf_hal::delay::Delay;
use esp_idf_svc::log::EspLogger;
use log::LevelFilter;


struct DS1302 {
    clk: PinDriver<'static, Gpio1, Output>,
    dat: Option<PinDriver<'static, Gpio2, Output>>,
    rst: PinDriver<'static, Gpio3, Output>,
    hour: u8,
    minute: u8,
    second: u8,
    delay: Delay
}

#[allow(dead_code)]
impl DS1302 {
    /// Create a new instance of DS1302 RTC
    fn new(clk: PinDriver<'static, Gpio1, Output>, dat: PinDriver<'static, Gpio2, Output>, rst: PinDriver<'static, Gpio3, Output>, delay: Delay) -> Self {
        Self {
            clk,
            dat: Some(dat),
            rst,
            hour: 0,
            minute: 0,
            second: 0,
            delay
        }
    }

    /// Set the time to hours:minutes:seconds
    fn init(&mut self, hour: u8, minute: u8, second: u8) {
        let sclk = &mut self.clk;
        let io = self.dat.as_mut().unwrap();
        let ce = &mut self.rst;
        let ho = Self::dec_to_bcd(hour);
        let min = Self::dec_to_bcd(minute);
        let sec = Self::dec_to_bcd(second);

        // DISABLE WRITE PROTECTION //
        ce.set_low().unwrap();
        // start transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x8E on control register using IO pin
        io.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x8E >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // send data 0x00 to disable write protection
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x00 >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // end transaction
        ce.set_low().unwrap();

        // SET SECONDS (CLEAR CH BIT) //
        // start transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x80 on control register using IO pin
        io.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x80 >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // send data seconds and clear bit 7
        let sec = sec & 0x7F;
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (sec >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // end transaction
        ce.set_low().unwrap();

        // SET MINUTES //
        // start transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x82 on control register using IO pin
        io.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x82 >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // send data minutes
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (min >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        ce.set_low().unwrap();

        // SET HOURS (24H MODE) //
        // begin transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        // write byte 0x84 on control register using IO pin
        io.set_low().unwrap();
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0x84 >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // send data hours and set bit 7 to 0 for 24h mode
        let hrs = ho & 0x3F;
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (hrs >> i) & 0x01 == 1 { io.set_high().unwrap(); } else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }
        // end transaction
        ce.set_low().unwrap();
    }

    /// Read the clock updates the time fields
    fn read(&mut self) {
        let sclk = &mut self.clk;
        let io = self.dat.as_mut().unwrap();
        let ce = &mut self.rst;

        io.set_low().unwrap();
        // begin transaction
        sclk.set_low().unwrap();
        ce.set_high().unwrap();
        self.delay.delay_us(1);
        // send burst read command 0xBF
        for i in 0..8 {
            sclk.set_low().unwrap();
            if (0xBF >> i) & 0x01 == 1 { io.set_high().unwrap(); }
            else { io.set_low().unwrap(); }
            self.delay.delay_us(1);
            sclk.set_high().unwrap();
            self.delay.delay_us(1);
        }

        // set io_pin to input mode
        let output_pin = self.dat.take().unwrap();
        let input_pin = output_pin.into_input().unwrap();

        // read seconds, minutes, and hours
        let mut sec_bcd = 0;
        for i in 0..8 {
            sclk.set_low().unwrap();
            let bit = if input_pin.is_high() { 1 } else { 0 };
            sec_bcd |= bit << i;
            sclk.set_high().unwrap();
        }
        let mut min_bcd = 0;
        for i in 0..8 {
            sclk.set_low().unwrap();
            let bit = if input_pin.is_high() { 1 } else { 0 };
            min_bcd |= bit << i;
            sclk.set_high().unwrap();
        }
        let mut hr_bcd = 0;
        for i in 0..8 {
            sclk.set_low().unwrap();
            let bit = if input_pin.is_high() { 1 } else { 0 };
            hr_bcd |= bit << i;
            sclk.set_high().unwrap();
        }
        // skip unused registers (date, month, year, etc)
        for _ in 0..(8 * 4) {
            sclk.set_low().unwrap();
            let _ = if input_pin.is_high() { 1 } else { 0 };
            sclk.set_high().unwrap();
        }
        // end transaction
        ce.set_low().unwrap();

        // set io_pin back to output mode
        let output_pin = input_pin.into_output().unwrap();
        self.dat = Some(output_pin);

        // translate values, update fields
        self.second = Self::bcd_to_dec(sec_bcd & 0x7F);
        self.minute = Self::bcd_to_dec(min_bcd & 0x7F);
        self.hour = Self::bcd_to_dec(hr_bcd & 0x3F);
    }

    /// Convert Binary Coded Decimal to Decimal
    fn bcd_to_dec(bcd: u8) -> u8 {
        ((bcd >> 4)*10) + (bcd & 0x0F)
    }
    /// Convert Decimal to Binary Coded Decimal
    fn dec_to_bcd(dec: u8) -> u8 {
        ((dec/10) << 4) | (dec%10)
    }
}


/// Example of using DS1302 RTC
/// This example sets the RTC to 12:34:56 and reads the time every second
/// The CLK, DAT, and RST pins are set to GPIO1, GPIO2, and GPIO3 respectively, adapt as needed
fn main() {
    // set up logging to not spam gpio messages in the log
    let logger = EspLogger::new();
    logger.initialize();
    let _ = logger.set_target_level("gpio", LevelFilter::Warn);
    let _ = log::set_logger(Box::leak(Box::new(logger)));
    esp_idf_svc::sys::link_patches();

    // declare peripherals and delay
    let peripherals = Peripherals::take().unwrap();
    let delay = Delay::default();

    // setup RTC pins
    let clk_pin = PinDriver::output(peripherals.pins.gpio1).unwrap();
    let dat_pin = PinDriver::output(peripherals.pins.gpio2).unwrap();
    let rst_pin = PinDriver::output(peripherals.pins.gpio3).unwrap();

    // create and initialize DS1302 RTC to 12:34:56
    let mut rtc = DS1302::new(clk_pin, dat_pin, rst_pin, delay);
    //rtc.init(12, 34, 56); // once the RTC is set comment this before the next upload

    loop {
        rtc.read();
        log::info!("Time: {:02}:{:02}:{:02}", rtc.hour, rtc.minute, rtc.second);

        delay.delay_ms(1000);
    }
}
