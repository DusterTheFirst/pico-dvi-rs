use embedded_hal::PwmPin;
use rp_pico::{
    hal::{
        gpio::{
            FunctionPio0, FunctionPwm, OutputDriveStrength, OutputOverride, OutputSlewRate, Pin,
            PinId, PullDown,
        },
        pio::{
            self, InstalledProgram, PIOBuilder, PinDir, Running, StateMachine, StateMachineGroup3,
            StateMachineIndex, Stopped, Tx, UninitStateMachine, ValidStateMachine,
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
    pub red_pos: Pin<RedPos, FunctionPio0, PullDown>,
    pub red_neg: Pin<RedNeg, FunctionPio0, PullDown>,

    pub green_pos: Pin<GreenPos, FunctionPio0, PullDown>,
    pub green_neg: Pin<GreenNeg, FunctionPio0, PullDown>,

    pub blue_pos: Pin<BluePos, FunctionPio0, PullDown>,
    pub blue_neg: Pin<BlueNeg, FunctionPio0, PullDown>,
}

pub struct DviClockPins<SliceId, Pos, Neg>
where
    SliceId: pwm::SliceId,
    Pos: PinId + ValidPwmOutputPin<SliceId, pwm::A>,
    Neg: PinId + ValidPwmOutputPin<SliceId, pwm::B>,
{
    pub clock_pos: Pin<Pos, FunctionPwm, PullDown>, // TODO: allow different order?
    pub clock_neg: Pin<Neg, FunctionPwm, PullDown>,
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
    Pos: PinId + ValidPwmOutputPin<SliceId, pwm::A>,
    Neg: PinId + ValidPwmOutputPin<SliceId, pwm::B>,
    RedPos: PinId,
    RedNeg: PinId,
    GreenPos: PinId,
    GreenNeg: PinId,
    BluePos: PinId,
    BlueNeg: PinId,
{
    pio: pio::PIO<PIO>, // FIXME:
    data_pins: DviDataPins<RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>, // FIXME:
    clock_pins: DviClockPins<SliceId, Pos, Neg>,

    state_machines: StateMachineState<PIO>,
    tx_fifo: (
        Tx<(PIO, pio::SM0)>,
        Tx<(PIO, pio::SM1)>,
        Tx<(PIO, pio::SM2)>,
    ),
}

enum StateMachineState<PIO: pio::PIOExt> {
    Stopped(StateMachineGroup3<PIO, pio::SM0, pio::SM1, pio::SM2, Stopped>),
    Running(StateMachineGroup3<PIO, pio::SM0, pio::SM1, pio::SM2, Running>),
    Taken,
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
    ClockPos: PinId + ValidPwmOutputPin<SliceId, pwm::A>,
    ClockNeg: PinId + ValidPwmOutputPin<SliceId, pwm::B>,
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
        pos_pin: &mut Pin<Pos, FunctionPio0, PullDown>,
        neg_pin: &mut Pin<Neg, FunctionPio0, PullDown>,
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
            OutputOverride::DontInvert
        } else {
            OutputOverride::Invert
        };

        let (mut state_machine, _, tx) = PIOBuilder::from_program(unsafe { program.share() })
            .side_set_pin_base(negative_id.min(positive_id))
            .clock_divisor_fixed_point(1, 0)
            .autopull(true)
            .buffers(pio::Buffers::OnlyTx)
            .pull_threshold(20)
            .out_shift_direction(pio::ShiftDirection::Right)
            .build(state_machine);

        state_machine.set_pindirs([(negative_id, PinDir::Output), (positive_id, PinDir::Output)]);

        neg_pin.set_drive_strength(OutputDriveStrength::TwoMilliAmps);
        neg_pin.set_slew_rate(OutputSlewRate::Slow);
        neg_pin.set_output_override(output_override);

        pos_pin.set_drive_strength(OutputDriveStrength::TwoMilliAmps);
        pos_pin.set_slew_rate(OutputSlewRate::Slow);
        pos_pin.set_output_override(output_override);

        (state_machine, tx)
    }

    pub fn new(
        pio: PIO,
        resets: &mut pac::RESETS,
        mut data_pins: DviDataPins<RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>,
        mut clock_pins: DviClockPins<SliceId, ClockPos, ClockNeg>,
    ) -> Self {
        let (mut pio, state_machine_blue, state_machine_green, state_machine_red, _) =
            pio.split(resets);

        // 3 PIO state machines to drive 6 data lines
        let dvi_output_program = pio_proc::pio_file!("src/dvi_differential.pio");

        let installed_program = pio.install(&dvi_output_program.program).unwrap();

        // TODO: do not consume pins?
        let (state_machine_blue, tx_blue) = Self::configure_state_machine::<BluePos, BlueNeg, _>(
            &installed_program,
            state_machine_blue,
            &mut data_pins.blue_pos,
            &mut data_pins.blue_neg,
        );

        let (state_machine_green, tx_green) = Self::configure_state_machine::<GreenPos, GreenNeg, _>(
            &installed_program,
            state_machine_green,
            &mut data_pins.green_pos,
            &mut data_pins.green_neg,
        );

        let (state_machine_red, tx_red) = Self::configure_state_machine::<RedPos, RedNeg, _>(
            &installed_program,
            state_machine_red,
            &mut data_pins.red_pos,
            &mut data_pins.red_neg,
        );

        // DVI clock driven
        let clock_pwm = &mut clock_pins.pwm_slice;
        clock_pwm.default_config();
        clock_pwm.set_top(9);

        clock_pwm.channel_a.clr_inverted();
        clock_pwm.channel_a.set_duty(5);
        let mut clock_pos = clock_pwm.channel_a.output_to(clock_pins.clock_pos);
        clock_pos.set_drive_strength(OutputDriveStrength::TwoMilliAmps);
        clock_pos.set_slew_rate(OutputSlewRate::Slow);

        clock_pwm.channel_b.set_inverted();
        clock_pwm.channel_b.set_duty(5);
        let mut clock_neg = clock_pwm.channel_b.output_to(clock_pins.clock_neg);
        clock_neg.set_drive_strength(OutputDriveStrength::TwoMilliAmps);
        clock_neg.set_slew_rate(OutputSlewRate::Slow);
        clock_pwm.enable();

        Self {
            pio,
            data_pins,
            clock_pins: DviClockPins {
                clock_pos,
                clock_neg,
                pwm_slice: clock_pins.pwm_slice,
            },
            state_machines: StateMachineState::Stopped(
                state_machine_blue
                    .with(state_machine_green)
                    .with(state_machine_red),
            ),
            tx_fifo: (tx_blue, tx_green, tx_red),
        }
    }

    pub fn tx(
        &self,
    ) -> (
        &Tx<(PIO, pio::SM0)>,
        &Tx<(PIO, pio::SM1)>,
        &Tx<(PIO, pio::SM2)>,
    ) {
        (&self.tx_fifo.0, &self.tx_fifo.1, &self.tx_fifo.2)
    }

    pub fn wait_fifos_full(&self) {
        wait_fifo_full(&self.tx_fifo.0);
        wait_fifo_full(&self.tx_fifo.1);
        wait_fifo_full(&self.tx_fifo.2);
    }

    pub fn enable(&mut self) {
        if let StateMachineState::Stopped(state_machines) =
            core::mem::replace(&mut self.state_machines, StateMachineState::Taken)
        {
            let state_machines = state_machines.sync().start();
            self.state_machines = StateMachineState::Running(state_machines);
        }
        self.clock_pins.pwm_slice.enable();
    }
}

fn wait_fifo_full<SM: ValidStateMachine>(fifo: &Tx<SM>) {
    while !fifo.is_full() {
        core::hint::spin_loop()
    }
}
