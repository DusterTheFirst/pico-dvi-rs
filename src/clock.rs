use fugit::RateExtU32;
use rp_pico::{
    hal::{
        clocks::{ClockSource, ClocksManager},
        pll::{
            self, common_configs::PLL_USB_48MHZ, setup_pll_blocking, PLLConfig, PhaseLockedLoop,
        },
        rosc::RingOscillator,
        xosc::{self, setup_xosc_blocking, CrystalOscillator},
        Clock, Watchdog,
    },
    pac, XOSC_CRYSTAL_FREQ,
};

/// Since we need to overclock the pico, we need to set these clocks up ourselves
pub fn init_clocks(
    xosc: pac::XOSC,
    rosc: pac::ROSC,
    clocks: pac::CLOCKS,
    pll_sys: pac::PLL_SYS,
    pll_usb: pac::PLL_USB,
    resets: &mut pac::RESETS,
    watchdog: &mut Watchdog,
) -> ClocksManager {
    // Enable the xosc
    let xosc = setup_xosc_blocking(xosc, XOSC_CRYSTAL_FREQ.Hz())
        .expect("crystal oscillator should be configured");

    let rosc = RingOscillator::new(rosc).initialize();

    // Start tick in watchdog
    watchdog.enable_tick_generation((XOSC_CRYSTAL_FREQ / 1_000_000) as u8);

    let mut clocks = ClocksManager::new(clocks);

    // INFO: Overclock to 10 * 25.175 MHz ~= 252 MHz for mandatory minimum DVI output resolution: VGA (640x480) @ 60 Hz
    // Section following comes from https://docs.rs/rp2040-hal/latest/rp2040_hal/clocks/index.html#usage-extended

    // Configure PLLs
    //                   REF     FBDIV VCO            POSTDIV
    // PLL SYS: 12 / 1 = 12MHz * 125 = 1512MHZ / 6 / 1 = 252MHz
    // PLL USB: 12 / 1 = 12MHz * 40  = 480 MHz / 5 / 2 =  48MHz
    let pll_sys = setup_pll_blocking(
        pll_sys,
        xosc.operating_frequency(),
        // Gotten from vcocalc.py
        PLLConfig {
            vco_freq: 1512.MHz(),
            refdiv: 1,
            post_div1: 6,
            post_div2: 1,
        },
        &mut clocks,
        resets,
    )
    .expect("sys pll should be configured");
    let pll_usb = setup_pll_blocking(
        pll_usb,
        xosc.operating_frequency(),
        PLL_USB_48MHZ,
        &mut clocks,
        resets,
    )
    .expect("sys pll should be configured");

    let clocks = configure_clocks(clocks, xosc, pll_sys, pll_usb);

    // Disable Ring Oscillator
    rosc.disable();

    clocks
}

fn configure_clocks(
    mut clocks: ClocksManager,
    xosc: CrystalOscillator<xosc::Stable>,
    pll_sys: PhaseLockedLoop<pll::Locked, pac::PLL_SYS>,
    pll_usb: PhaseLockedLoop<pll::Locked, pac::PLL_USB>,
) -> ClocksManager {
    // Configure clocks
    // CLK_REF = XOSC (12MHz) / 1 = 12MHz
    clocks
        .reference_clock
        .configure_clock(&xosc, xosc.get_freq())
        .unwrap();

    // CLK SYS = PLL SYS (125MHz) / 1 = 125MHz
    clocks
        .system_clock
        .configure_clock(&pll_sys, pll_sys.get_freq())
        .unwrap();

    // CLK USB = PLL USB (48MHz) / 1 = 48MHz
    clocks
        .usb_clock
        .configure_clock(&pll_usb, pll_usb.get_freq())
        .unwrap();

    // CLK ADC = PLL USB (48MHZ) / 1 = 48MHz
    clocks
        .adc_clock
        .configure_clock(&pll_usb, pll_usb.get_freq())
        .unwrap();

    // CLK RTC = PLL USB (48MHz) / 1024 = 46875Hz
    clocks
        .rtc_clock
        .configure_clock(&pll_usb, 46875u32.Hz())
        .unwrap();

    // CLK PERI = clk_sys. Used as reference clock for Peripherals. No dividers so just select and enable
    // Normally choose clk_sys or clk_usb
    clocks
        .peripheral_clock
        .configure_clock(&clocks.system_clock, 12.MHz())
        .unwrap();

    clocks
}
