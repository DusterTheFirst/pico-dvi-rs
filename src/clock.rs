use crate::hal::{
    clocks::{ClockSource, ClocksManager},
    fugit::{KilohertzU32, MegahertzU32, RateExtU32},
    pac,
    pll::{
        self,
        common_configs::{PLL_SYS_150MHZ, PLL_USB_48MHZ},
        setup_pll_blocking, PLLConfig, PhaseLockedLoop,
    },
    rosc::RingOscillator,
    xosc::{self, setup_xosc_blocking, CrystalOscillator},
    Clock, Watchdog,
};

struct ClockCfg {
    vco_freq: KilohertzU32,
    post_div1: u32,
    post_div2: u32,
}

// Values taken from pico-sdk hardware_pll/include/hardware/pll.h
const PICO_PLL_VCO_MIN_FREQ: MegahertzU32 = MegahertzU32::MHz(750);
const PICO_PLL_VCO_MAX_FREQ: MegahertzU32 = MegahertzU32::MHz(1600);
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

/// Determine PLL parameters for target frequency
///
/// Logic is adapted from check_sys_clock_khz in pico-sdk
#[doc(alias = "check_sys_clock_khz", alias = "vcocalc")]
fn configure_sys_clock(requested_freq: KilohertzU32) -> Option<ClockCfg> {
    let crystal_freq: KilohertzU32 = XOSC_CRYSTAL_FREQ.Hz();

    // Its called a feedback divider but it really is a clock multiplier
    // see 2.18.2 in rp2040-datasheet.pdf
    for feedback_divider in (16..=320).rev() {
        let vco_freq: KilohertzU32 = feedback_divider * crystal_freq;

        // Stop the loop since all consecutive numbers will also be less than this
        if vco_freq < PICO_PLL_VCO_MIN_FREQ {
            break;
        }

        if vco_freq > PICO_PLL_VCO_MAX_FREQ {
            continue;
        }

        for post_div1 in (1..=7).rev() {
            for post_div2 in (1..=post_div1).rev() {
                let divider = post_div1 * post_div2;

                let output_frequency: KilohertzU32 = vco_freq / divider;

                // Doing this instead of % to work around https://github.com/korken89/fugit/issues/41
                // Ensure the vco_freq is divisible by the clock dividers
                let vco_freq_divisible = output_frequency * divider == vco_freq;

                if output_frequency == requested_freq && vco_freq_divisible {
                    return Some(ClockCfg {
                        vco_freq,
                        post_div1,
                        post_div2,
                    });
                }
            }
        }
    }

    None
}

/// Since we need to overclock the pico, we need to set these clocks up ourselves
pub fn init_clocks(
    xosc: pac::XOSC,
    rosc: pac::ROSC,
    clocks: pac::CLOCKS,
    pll_sys: pac::PLL_SYS,
    pll_usb: pac::PLL_USB,
    resets: &mut pac::RESETS,
    watchdog: &mut Watchdog,
    freq_khz: KilohertzU32,
    hstx_divisor: u32,
) -> ClocksManager {
    // Enable the xosc
    let xosc = setup_xosc_blocking(xosc, XOSC_CRYSTAL_FREQ.Hz())
        .expect("crystal oscillator should be configured");

    let rosc = RingOscillator::new(rosc).initialize();

    // Start tick in watchdog
    watchdog.enable_tick_generation((XOSC_CRYSTAL_FREQ / 1_000_000) as u16);

    let mut clocks = ClocksManager::new(clocks);

    let clk_cfg = configure_sys_clock(freq_khz);
    let pll_config = match clk_cfg {
        Some(ClockCfg {
            vco_freq,
            post_div1,
            post_div2,
        }) => PLLConfig {
            vco_freq: vco_freq.convert(),
            refdiv: 1,
            post_div1: post_div1 as u8,
            post_div2: post_div2 as u8,
        },
        None => PLL_SYS_150MHZ,
    };

    // INFO: Overclock to 10 * 25.175 MHz ~= 252 MHz for mandatory minimum DVI output resolution: VGA (640x480) @ 60 Hz
    // Section following comes from https://docs.rs/rp2040-hal/latest/rp2040_hal/clocks/index.html#usage-extended

    // Configure PLLs
    //                   REF     FBDIV VCO            POSTDIV
    // PLL SYS: 12 / 1 = 12MHz * 125 = 1512MHZ / 6 / 1 = 252MHz
    // PLL USB: 12 / 1 = 12MHz * 40  = 480 MHz / 5 / 2 =  48MHz
    let pll_sys = setup_pll_blocking(
        pll_sys,
        xosc.operating_frequency(),
        pll_config,
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

    let clocks = configure_clocks(clocks, xosc, pll_sys, pll_usb, hstx_divisor);

    // Disable Ring Oscillator
    rosc.disable();

    clocks
}

fn configure_clocks(
    mut clocks: ClocksManager,
    xosc: CrystalOscillator<xosc::Stable>,
    pll_sys: PhaseLockedLoop<pll::Locked, pac::PLL_SYS>,
    pll_usb: PhaseLockedLoop<pll::Locked, pac::PLL_USB>,
    hstx_divisor: u32,
) -> ClocksManager {
    clocks.init_default(&xosc, &pll_sys, &pll_usb).unwrap();

    // CLK HSTX = same as system clock
    clocks
        .hstx_clock
        .configure_clock(
            &clocks.system_clock,
            clocks.system_clock.get_freq() / hstx_divisor,
        )
        .unwrap();

    clocks
}
