use embedded_hal::PwmPin;
use rp_pico::{
    hal::{
        gpio::{
            self, bank0, Disabled, FunctionPwm, OutputDriveStrength, OutputOverride,
            OutputSlewRate, Pin, PinId, PinMode, PullDown, ValidPinMode,
        },
        pio::{
            self, InstalledProgram, PIOBuilder, StateMachine, StateMachineGroup3,
            StateMachineIndex, Stopped, Tx, UninitStateMachine,
        },
        pwm::{self, FreeRunning, Slice, ValidPwmOutputPin},
    },
    pac,
};

pub struct DviDataPins<RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>
where
    RedPos: PinId,
    RedNeg: PinId,
    GreenPos: PinId,
    GreenNeg: PinId,
    BluePos: PinId,
    BlueNeg: PinId,
{
    pub red_pos: Pin<RedPos, gpio::Disabled<PullDown>>,
    pub red_neg: Pin<RedNeg, gpio::Disabled<PullDown>>,

    pub green_pos: Pin<GreenPos, gpio::Disabled<PullDown>>,
    pub green_neg: Pin<GreenNeg, gpio::Disabled<PullDown>>,

    pub blue_pos: Pin<BluePos, gpio::Disabled<PullDown>>,
    pub blue_neg: Pin<BlueNeg, gpio::Disabled<PullDown>>,
}

pub struct DviClockPins<SliceId, Pos, Neg, Mode>
where
    SliceId: pwm::SliceId,
    Pos: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::A>,
    Neg: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::B>,
    Mode: PinMode + ValidPinMode<Pos> + ValidPinMode<Neg>,
{
    pub clock_pos: Pin<Pos, Mode>, // TODO: allow different order?
    pub clock_neg: Pin<Neg, Mode>,
    pub pwm_slice: Slice<SliceId, FreeRunning>,
}

pub struct DviSerializer<
    PIO,
    SliceId,
    Pos,
    Neg,
    RedPos,
    RedNeg,
    GreenPos,
    GreenNeg,
    BluePos,
    BlueNeg,
> where
    PIO: pio::PIOExt,
    SliceId: pwm::SliceId,
    Pos: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::A>,
    Neg: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::B>,
    RedPos: PinId,
    RedNeg: PinId,
    GreenPos: PinId,
    GreenNeg: PinId,
    BluePos: PinId,
    BlueNeg: PinId,
{
    pio: pio::PIO<PIO>, // FIXME:
    data_pins: DviDataPins<RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>, // FIXME:
    clock_pins: DviClockPins<SliceId, Pos, Neg, FunctionPwm>,

    state_machines: StateMachineGroup3<PIO, pio::SM0, pio::SM1, pio::SM2, Stopped>,
    tx_fifo: (
        Tx<(PIO, pio::SM0)>,
        Tx<(PIO, pio::SM1)>,
        Tx<(PIO, pio::SM2)>,
    ),
}

impl<PIO, SliceId, ClockPos, ClockNeg, RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>
    DviSerializer<
        PIO,
        SliceId,
        ClockPos,
        ClockNeg,
        RedPos,
        RedNeg,
        GreenPos,
        GreenNeg,
        BluePos,
        BlueNeg,
    >
where
    PIO: pio::PIOExt,
    SliceId: pwm::SliceId,
    ClockPos: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::A>,
    ClockNeg: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::B>,
    RedPos: PinId,
    RedNeg: PinId,
    GreenPos: PinId,
    GreenNeg: PinId,
    BluePos: PinId,
    BlueNeg: PinId,
{
    fn configure_state_machine<Pos, Neg, SM>(
        program: &InstalledProgram<PIO>,
        state_machine: UninitStateMachine<(PIO, SM)>,
        pos_pin: &mut Pin<Pos, gpio::Disabled<PullDown>>,
        neg_pin: &mut Pin<Neg, gpio::Disabled<PullDown>>,
    ) -> (StateMachine<(PIO, SM), Stopped>, Tx<(PIO, SM)>)
    where
        Pos: PinId,
        Neg: PinId,
        SM: StateMachineIndex,
    {
        let positive_id = pos_pin.id().num;
        let negative_id = neg_pin.id().num;

        defmt::assert_eq!(
            negative_id.abs_diff(positive_id),
            1,
            "differential pins must be sequential"
        );

        // Invert pin outputs if in other order
        let output_override = if positive_id < negative_id {
            OutputOverride::Invert
        } else {
            OutputOverride::DontInvert
        };

        let (state_machine, _, tx) = PIOBuilder::from_program(unsafe { program.share() })
            .side_set_pin_base(negative_id.min(positive_id))
            .clock_divisor_fixed_point(1, 1)
            .autopull(true)
            .buffers(pio::Buffers::OnlyRx)
            .pull_threshold(8)
            .build(state_machine);

        neg_pin.set_drive_strength(OutputDriveStrength::TwoMilliAmps);
        neg_pin.set_slew_rate(OutputSlewRate::Slow);
        neg_pin.set_output_override(output_override);

        pos_pin.set_drive_strength(OutputDriveStrength::TwoMilliAmps);
        neg_pin.set_slew_rate(OutputSlewRate::Slow);
        pos_pin.set_output_override(output_override);

        (state_machine, tx)
    }

    pub fn new(
        pio: PIO,
        resets: &mut pac::RESETS,
        mut data_pins: DviDataPins<RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>,
        mut clock_pins: DviClockPins<SliceId, ClockPos, ClockNeg, Disabled<PullDown>>,
    ) -> Self {
        let (mut pio, state_machine_red, state_machine_green, state_machine_blue, _) =
            pio.split(resets);

        // 3 PIO state machines to drive 6 data lines
        // 10 Red +
        // 11 Red -
        // 12 Green +
        // 13 Green -
        // 14 Blue +
        // 15 Blue -
        let dvi_output_program = pio_proc::pio_file!("src/dvi_differential.pio");

        let installed_program = pio.install(&dvi_output_program.program).unwrap();

        // TODO: do not consume pins?
        let (state_machine_red, tx_red) = Self::configure_state_machine::<RedPos, RedNeg, _>(
            &installed_program,
            state_machine_red,
            &mut data_pins.red_pos,
            &mut data_pins.red_neg,
        );

        let (state_machine_green, tx_green) = Self::configure_state_machine::<GreenPos, GreenNeg, _>(
            &installed_program,
            state_machine_green,
            &mut data_pins.green_pos,
            &mut data_pins.green_neg,
        );

        let (state_machine_blue, tx_blue) = Self::configure_state_machine::<BluePos, BlueNeg, _>(
            &installed_program,
            state_machine_blue,
            &mut data_pins.blue_pos,
            &mut data_pins.blue_neg,
        );

        // DVI clock driven by PWM4
        // 8 CLK +
        // 9 CLK -
        let clock_pwm = &mut clock_pins.pwm_slice;
        clock_pwm.default_config();
        clock_pwm.disable();
        clock_pwm.set_top(9);

        clock_pwm.channel_a.clr_inverted();
        clock_pwm.channel_a.set_duty(5);
        let mut clock_pos = clock_pwm.channel_a.output_to(clock_pins.clock_pos);
        clock_pos.set_drive_strength(OutputDriveStrength::TwelveMilliAmps);
        clock_pos.set_slew_rate(OutputSlewRate::Fast);

        clock_pwm.channel_b.set_inverted();
        clock_pwm.channel_b.set_duty(5);
        let mut clock_neg = clock_pwm.channel_b.output_to(clock_pins.clock_neg);
        clock_neg.set_drive_strength(OutputDriveStrength::TwelveMilliAmps);
        clock_neg.set_slew_rate(OutputSlewRate::Fast);

        // TODO: DMA
        // TODO: TMDS ENCODING

        Self {
            pio,
            data_pins,
            clock_pins: DviClockPins {
                clock_pos,
                clock_neg,
                pwm_slice: clock_pins.pwm_slice,
            },
            state_machines: state_machine_red
                .with(state_machine_green)
                .with(state_machine_blue),
            tx_fifo: (tx_red, tx_green, tx_blue),
        }
    }

    pub fn enable(mut self) {
        let state_machines = self.state_machines.sync().start();
        self.clock_pins.pwm_slice.enable();

        // TODO: TMDS LANES
        // TODO: DMA
        // TODO: DVI typestate?
    }
}
