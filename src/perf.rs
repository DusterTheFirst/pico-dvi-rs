pub fn setup(mut sys_tick: rp_pico::pac::SYST) {
    sys_tick.disable_interrupt();

    unsafe { SYSTICK = Some(sys_tick) };
}

static mut SYSTICK: Option<rp_pico::pac::SYST> = None;

#[track_caller]
pub fn instrument<T>(function: impl FnOnce() -> T) -> T {
    let sys_tick = unsafe { SYSTICK.as_mut().unwrap() };

    sys_tick.set_reload(0x00ffffff);
    sys_tick.clear_current();
    sys_tick.enable_counter();

    let result = function();

    let cycles = rp_pico::pac::SYST::get_current();
    sys_tick.disable_counter();
    let wrapped = sys_tick.has_wrapped();

    let ms = (cycles * 10) / rp_pico::pac::SYST::get_ticks_per_10ms();

    let caller = core::panic::Location::caller();
    defmt::println!(
        "{=str}:{=u32} {=u32} cycles {=u32} ms {=bool}",
        caller.file(),
        caller.line(),
        cycles,
        ms,
        wrapped
    );

    result
}
