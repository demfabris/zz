#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spring {
    pub(crate) stiffness: f64,
    pub(crate) damping: f64,
    pub(crate) mass: f64,
    pub(crate) velocity: f64,
}

impl Spring {
    fn progress(&self, elapsed: f64) -> f64 {
        if elapsed <= 0.0 {
            return 0.0;
        }
        let omega = (self.stiffness / self.mass).sqrt();
        let zeta = self.damping / (2.0 * (self.stiffness * self.mass).sqrt());
        let velocity = self.velocity;
        let remaining = if (zeta - 1.0).abs() < 1e-3 {
            (-omega * elapsed).exp() * (1.0 + (omega - velocity) * elapsed)
        } else if zeta < 1.0 {
            let damped = omega * (1.0 - zeta * zeta).sqrt();
            (-zeta * omega * elapsed).exp()
                * ((damped * elapsed).cos()
                    + (zeta * omega - velocity) / damped * (damped * elapsed).sin())
        } else {
            let spread = omega * (zeta * zeta - 1.0).sqrt();
            let slow = -zeta * omega + spread;
            let fast = -zeta * omega - spread;
            let weight = (-velocity - fast) / (slow - fast);
            weight * (slow * elapsed).exp() + (1.0 - weight) * (fast * elapsed).exp()
        };
        1.0 - remaining
    }
}

struct Leg {
    delta: f64,
    started: f64,
    duration: f64,
    spring: Option<Spring>,
}

impl Leg {
    fn progress(&self, time: f64) -> f64 {
        if time >= self.started + self.duration {
            return 1.0;
        }
        self.spring
            .map_or(0.0, |spring| spring.progress(time - self.started))
    }
}

pub(crate) struct KeyboardMotion {
    from: f64,
    legs: Vec<Leg>,
}

impl KeyboardMotion {
    pub(crate) fn new(from: f64) -> Self {
        Self {
            from,
            legs: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, to: f64, started: f64, duration: f64, spring: Option<Spring>) {
        let delta = to - self.target();
        self.legs.push(Leg {
            delta,
            started,
            duration,
            spring,
        });
    }

    pub(crate) fn began(&mut self, started: f64) {
        if let Some(leg) = self.legs.last_mut()
            && (0.0..1.0).contains(&(started - leg.started))
        {
            leg.started = started;
        }
    }

    pub(crate) fn target(&self) -> f64 {
        self.from + self.legs.iter().map(|leg| leg.delta).sum::<f64>()
    }

    pub(crate) fn sampled(&self) -> bool {
        self.legs.iter().any(|leg| leg.spring.is_none())
    }

    pub(crate) fn value(&self, time: f64) -> f64 {
        self.from
            + self
                .legs
                .iter()
                .map(|leg| leg.delta * leg.progress(time))
                .sum::<f64>()
    }

    pub(crate) fn finished(&self, time: f64) -> bool {
        self.legs
            .iter()
            .all(|leg| time >= leg.started + leg.duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYBOARD: Spring = Spring {
        stiffness: 555.0265,
        damping: 47.118,
        mass: 1.0,
        velocity: 0.0,
    };

    #[test]
    fn the_keyboard_spring_starts_still_and_lands() {
        assert_eq!(KEYBOARD.progress(0.0), 0.0);
        assert!(KEYBOARD.progress(0.016) < 0.06);
        assert!(KEYBOARD.progress(0.1) > 0.6);
        assert!(KEYBOARD.progress(0.383) > 0.998);
        let mut previous = 0.0;
        for frame in 1..=46 {
            let progress = KEYBOARD.progress(f64::from(frame) / 120.0);
            assert!(progress >= previous);
            previous = progress;
        }
    }

    #[test]
    fn under_and_over_damped_springs_share_the_ends() {
        for damping in [10.0, 47.118, 200.0] {
            let spring = Spring {
                damping,
                ..KEYBOARD
            };
            assert_eq!(spring.progress(0.0), 0.0);
            assert!((spring.progress(10.0) - 1.0).abs() < 1e-6);
            assert!(spring.progress(0.001) < 0.01);
        }
    }

    #[test]
    fn a_motion_moves_from_its_start_to_its_target() {
        let mut motion = KeyboardMotion::new(0.0);
        motion.push(308.0, 1.0, 0.383, Some(KEYBOARD));
        assert_eq!(motion.target(), 308.0);
        assert_eq!(motion.value(1.0), 0.0);
        let middle = motion.value(1.1);
        assert!(middle > 150.0 && middle < 308.0);
        assert!(!motion.finished(1.3));
        assert!(motion.finished(1.383));
        assert_eq!(motion.value(1.383), 308.0);
    }

    #[test]
    fn a_new_target_adds_onto_the_running_motion() {
        let mut motion = KeyboardMotion::new(0.0);
        motion.push(300.0, 0.0, 0.383, Some(KEYBOARD));
        let before = motion.value(0.1);
        motion.push(340.0, 0.1, 0.383, Some(KEYBOARD));
        assert_eq!(motion.target(), 340.0);
        assert_eq!(motion.value(0.1), before);
        assert!(!motion.finished(0.4));
        assert_eq!(motion.value(0.483), 340.0);
        assert!(motion.finished(0.483));
    }

    #[test]
    fn a_reversal_heads_back_without_a_jump() {
        let mut motion = KeyboardMotion::new(0.0);
        motion.push(300.0, 0.0, 0.383, Some(KEYBOARD));
        let turn = motion.value(0.05);
        motion.push(0.0, 0.05, 0.383, Some(KEYBOARD));
        assert_eq!(motion.value(0.05), turn);
        assert_eq!(motion.value(0.433), 0.0);
    }

    #[test]
    fn the_newest_leg_follows_the_committed_begin_time() {
        let mut motion = KeyboardMotion::new(0.0);
        motion.push(308.0, 1.0, 0.383, Some(KEYBOARD));
        motion.began(1.01);
        assert_eq!(motion.value(1.01), 0.0);
        assert!(!motion.finished(1.385));
        assert!(motion.finished(1.393));
        motion.began(0.5);
        motion.began(5.0);
        assert!(motion.finished(1.393));
    }

    #[test]
    fn a_curve_that_is_not_a_spring_is_sampled() {
        let mut motion = KeyboardMotion::new(0.0);
        motion.push(300.0, 0.0, 0.25, None);
        assert!(motion.sampled());
        assert_eq!(motion.value(0.25), 300.0);
    }
}
