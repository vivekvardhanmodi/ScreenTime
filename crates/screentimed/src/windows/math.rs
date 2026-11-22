use screentime_core::IdleState;
use std::time::Duration;

/// Calculates elapsed idle time in milliseconds using 32-bit modular wrapping subtraction.
///
/// Win32 `GetLastInputInfo` returns a 32-bit millisecond tick count (`dwTime`)
/// which rolls over back to 0 every 2^32 ms (~49.71 days).
///
/// By using two's-complement wrapping subtraction (`current_ticks.wrapping_sub(last_input_ticks)`),
/// rollover is handled seamlessly without multi-day idle anomalies.
#[inline]
pub fn calculate_idle_duration_ms(current_ticks: u32, last_input_ticks: u32) -> u64 {
    current_ticks.wrapping_sub(last_input_ticks) as u64
}

/// Calculates elapsed idle time as a `Duration` with 32-bit rollover protection.
#[inline]
pub fn calculate_idle_time(current_ticks: u32, last_input_ticks: u32) -> Duration {
    Duration::from_millis(calculate_idle_duration_ms(current_ticks, last_input_ticks))
}

/// Computes the next adaptive polling interval for idle checking.
///
/// Polling strategy:
/// - When idle: poll every 1.0 second so resume from idle is detected promptly (<1.5s latency).
/// - When near idle threshold (within 5 seconds): poll every 1.0 second to ensure accurate boundary crossing.
/// - When active and far from threshold: poll every 2.0 seconds to minimize CPU wakeups and preserve C-states.
pub fn calculate_poll_interval(
    is_idle: bool,
    idle_duration: Duration,
    idle_threshold: Duration,
) -> Duration {
    if is_idle || idle_duration + Duration::from_secs(5) >= idle_threshold {
        Duration::from_millis(1000)
    } else {
        Duration::from_millis(2000)
    }
}

/// Evaluates state transitions between `Active` and `Idle`.
///
/// - Transition from `Active` to `Idle` occurs when `idle_duration >= idle_threshold`.
/// - Transition from `Idle` to `Active` occurs when `idle_duration < resume_threshold`.
/// - If neither threshold condition is satisfied, the current state is retained.
pub fn evaluate_idle_transition(
    current_state: IdleState,
    idle_duration: Duration,
    idle_threshold: Duration,
    resume_threshold: Duration,
) -> IdleState {
    match current_state {
        IdleState::Active => {
            if idle_duration >= idle_threshold {
                IdleState::Idle
            } else {
                IdleState::Active
            }
        }
        IdleState::Idle => {
            if idle_duration < resume_threshold {
                IdleState::Active
            } else {
                IdleState::Idle
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_math_normal_progression() {
        assert_eq!(calculate_idle_duration_ms(10_000, 5_000), 5_000);
        assert_eq!(calculate_idle_duration_ms(10_000, 10_000), 0);
        assert_eq!(
            calculate_idle_time(50_000, 20_000),
            Duration::from_millis(30_000)
        );
    }

    #[test]
    fn test_tick_math_32bit_rollover() {
        // Ticks just before 49.7 day rollover (0xFFFF_FFF0)
        // Current ticks just after rollover (0x0000_0020)
        // Elapsed = 16 ms before rollover + 32 ms after rollover = 48 ms.
        let last_input = 0xFFFF_FFF0_u32;
        let current = 0x0000_0020_u32;
        assert_eq!(calculate_idle_duration_ms(current, last_input), 48);
        assert_eq!(
            calculate_idle_time(current, last_input),
            Duration::from_millis(48)
        );

        // Another boundary case: last_input is at max u32, current is 100 ms after rollover
        let last_max = u32::MAX;
        let current_100 = 99_u32;
        assert_eq!(calculate_idle_duration_ms(current_100, last_max), 100);
    }

    #[test]
    fn test_adaptive_poll_intervals() {
        let threshold = Duration::from_secs(90);

        // When idle, always 1s
        assert_eq!(
            calculate_poll_interval(true, Duration::from_secs(120), threshold),
            Duration::from_millis(1000)
        );
        assert_eq!(
            calculate_poll_interval(true, Duration::from_secs(0), threshold),
            Duration::from_millis(1000)
        );

        // When active and far from threshold (e.g. 10s idle on 90s threshold) -> 2s
        assert_eq!(
            calculate_poll_interval(false, Duration::from_secs(10), threshold),
            Duration::from_millis(2000)
        );

        // When active and near threshold (e.g. 86s idle on 90s threshold) -> 1s
        assert_eq!(
            calculate_poll_interval(false, Duration::from_secs(86), threshold),
            Duration::from_millis(1000)
        );
    }

    #[test]
    fn test_idle_transitions() {
        let idle_thresh = Duration::from_secs(60);
        let resume_thresh = Duration::from_millis(1500);

        // Active state staying active
        assert_eq!(
            evaluate_idle_transition(
                IdleState::Active,
                Duration::from_secs(30),
                idle_thresh,
                resume_thresh
            ),
            IdleState::Active
        );

        // Active state transitioning to Idle at threshold
        assert_eq!(
            evaluate_idle_transition(
                IdleState::Active,
                Duration::from_secs(60),
                idle_thresh,
                resume_thresh
            ),
            IdleState::Idle
        );

        // Idle state staying Idle
        assert_eq!(
            evaluate_idle_transition(
                IdleState::Idle,
                Duration::from_secs(70),
                idle_thresh,
                resume_thresh
            ),
            IdleState::Idle
        );

        // Idle state resuming to Active upon user input (idle duration < 1.5s)
        assert_eq!(
            evaluate_idle_transition(
                IdleState::Idle,
                Duration::from_millis(200),
                idle_thresh,
                resume_thresh
            ),
            IdleState::Active
        );
    }
}
