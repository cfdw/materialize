// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! A time interval abstract data type.

use std::fmt::{self, Write};

use failure::bail;
use serde::{Deserialize, Serialize};

use crate::adt::datetime::DateTimeField;

/// An interval of time meant to express SQL intervals.
///
/// Either a concrete number of seconds, or an abstract number of months.
///
/// Obtained by parsing an `INTERVAL '<value>' <unit> [TO <precision>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Hash, Deserialize)]
pub struct Interval {
    /// A possibly negative number of months for field types like `YEAR`
    pub months: i64,
    /// An actual timespan, possibly negative, because why not
    pub duration: std::time::Duration,
    pub is_positive_dur: bool,
}

impl Default for Interval {
    fn default() -> Self {
        Self {
            months: 0,
            duration: std::time::Duration::default(),
            is_positive_dur: true,
        }
    }
}

impl std::ops::Add for Interval {
    type Output = Self;
    // Since durations can only be positive, we need subtraction and boolean
    // operators inside the Add impl
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn add(self, other: Self) -> Self {
        let (is_positive_dur, duration) = if self.is_positive_dur == other.is_positive_dur {
            (self.is_positive_dur, self.duration + other.duration)
        } else if self.duration > other.duration {
            (self.is_positive_dur, self.duration - other.duration)
        } else {
            (other.is_positive_dur, other.duration - self.duration)
        };

        Self {
            months: self.months + other.months,
            duration,
            is_positive_dur,
        }
    }
}

impl std::ops::Sub for Interval {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        self + -other
    }
}

impl std::ops::Neg for Interval {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            months: -self.months,
            duration: self.duration,
            is_positive_dur: !self.is_positive_dur,
        }
    }
}

impl Interval {
    /// Computes the year part of the interval.
    ///
    /// The year part is the number of whole years in the interval. For example,
    /// this function returns `3.0` for the interval `3 years 4 months`.
    pub fn years(&self) -> f64 {
        (self.months / 12) as f64
    }

    /// Computes the month part of the interval.
    ///
    /// The whole part is the number of whole months in the interval, modulo 12.
    /// For example, this function returns `4.0` for the interval `3 years 4
    /// months`.
    pub fn months(&self) -> f64 {
        (self.months % 12) as f64
    }

    /// Computes the day part of the interval.
    ///
    /// The day part is the number of whole days in the interval. For example,
    /// this function returns `5.0` for the interval `5 days 4 hours 3 minutes
    /// 2.1 seconds`.
    pub fn days(&self) -> f64 {
        (self.duration.as_secs() / (60 * 60 * 24)) as f64
    }

    /// Computes the hour part of the interval.
    ///
    /// The hour part is the number of whole hours in the interval, modulo 24.
    /// For example, this function returns `4.0` for the interval `5 days 4
    /// hours 3 minutes 2.1 seconds`.
    pub fn hours(&self) -> f64 {
        ((self.duration.as_secs() / (60 * 60)) % 24) as f64
    }

    /// Computes the minute part of the interval.
    ///
    /// The minute part is the number of whole minutes in the interval, modulo
    /// 60. For example, this function returns `3.0` for the interval `5 days 4
    /// hours 3 minutes 2.1 seconds`.
    pub fn minutes(&self) -> f64 {
        ((self.duration.as_secs() / 60) % 60) as f64
    }

    /// Computes the second part of the interval.
    ///
    /// The second part is the number of fractional seconds in the interval,
    /// modulo 60.0.
    pub fn seconds(&self) -> f64 {
        let s = (self.duration.as_secs() % 60) as f64;
        let ns = f64::from(self.duration.subsec_nanos()) / 1e9;
        s + ns
    }

    /// Computes the nanosecond part of the interval.
    pub fn nanoseconds(&self) -> i64 {
        if self.is_positive_dur {
            self.duration.subsec_nanos() as i64
        } else {
            -(self.duration.subsec_nanos() as i64)
        }
    }

    /// Computes the total number of seconds in the interval.
    pub fn as_seconds(&self) -> f64 {
        (self.months as f64) * 60.0 * 60.0 * 24.0 * 30.0
            + (self.duration.as_secs() as f64)
            + f64::from(self.duration.subsec_micros()) / 1e6
    }

    /// Truncate the "head" of the interval, removing all time units greater than `f`.
    pub fn truncate_high_fields(&mut self, f: DateTimeField) {
        use std::time::Duration;
        match f {
            DateTimeField::Year => {}
            DateTimeField::Month => self.months %= 12,
            DateTimeField::Day => self.months = 0,
            hms => {
                self.months = 0;
                self.duration = Duration::new(
                    self.duration.as_secs() % seconds_multiplier(hms.next_largest()),
                    self.duration.subsec_nanos(),
                );
            }
        }
    }

    /// Truncate the "tail" of the interval, removing all time units less than `f`.
    /// # Arguments
    /// - `f`: Round the interval down to the specified time unit.
    /// - `fsec_max_precision`: If `Some(x)`, keep only `x` places of nanosecond precision.
    ///    Must be `(0,6)`.
    ///
    /// # Errors
    /// - If `fsec_max_precision` is not None or within (0,6).
    pub fn truncate_low_fields(
        &mut self,
        f: DateTimeField,
        fsec_max_precision: Option<u64>,
    ) -> Result<(), failure::Error> {
        use std::time::Duration;
        use DateTimeField::*;
        match f {
            Year => {
                self.months -= self.months % 12;
                self.duration = Duration::new(0, 0);
            }
            Month => {
                self.duration = Duration::new(0, 0);
            }
            // Round nanoseconds.
            Second => {
                let default_precision = 6;
                let precision = match fsec_max_precision {
                    Some(p) => p,
                    None => default_precision,
                };

                if precision > default_precision {
                    bail!(
                        "SECOND precision must be (0, 6), have SECOND({})",
                        precision
                    )
                }

                let mut nanos = self.duration.subsec_nanos();

                // Check if value should round up to nearest fractional place.
                let remainder = nanos % 10_u32.pow(9 - precision as u32);
                if remainder / 10_u32.pow(8 - precision as u32) > 4 {
                    nanos += 10_u32.pow(9 - precision as u32);
                }

                self.duration = Duration::new(self.duration.as_secs(), nanos - remainder);
            }
            dhm => {
                self.duration = Duration::new(
                    self.duration.as_secs() - self.duration.as_secs() % (seconds_multiplier(dhm)),
                    0,
                );
            }
        }
        Ok(())
    }
}

/// Returns the number of seconds in a single unit of `field`.
fn seconds_multiplier(field: DateTimeField) -> u64 {
    use DateTimeField::*;
    match field {
        Day => 60 * 60 * 24,
        Hour => 60 * 60,
        Minute => 60,
        Second => 1,
        _other => unreachable!("Do not call with a non-duration field"),
    }
}

/// Format an interval in a human form
///
/// Example outputs:
///
/// * 1 year 2 months 5 days 03:04:00
/// * -1 year +5 days +18:59:29.3
/// * 00:00:00
impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut months = self.months;
        let neg_mos = months < 0;
        months = months.abs();
        let years = months / 12;
        months %= 12;
        let mut secs = self.duration.as_secs();
        let mut nanos = self.duration.subsec_nanos();
        let days = secs / (24 * 60 * 60);
        secs %= 24 * 60 * 60;
        let hours = secs / (60 * 60);
        secs %= 60 * 60;
        let minutes = secs / 60;
        secs %= 60;

        if years > 0 {
            if neg_mos {
                f.write_char('-')?;
            }
            write!(f, "{} year", years)?;
            if years > 1 {
                f.write_char('s')?;
            }
        }

        if months > 0 {
            if years != 0 {
                f.write_char(' ')?;
            }
            if neg_mos {
                f.write_char('-')?;
            }
            write!(f, "{} month", months)?;
            if months > 1 {
                f.write_char('s')?;
            }
        }

        if days > 0 {
            if years > 0 || months > 0 {
                f.write_char(' ')?;
            }
            if !self.is_positive_dur {
                f.write_char('-')?;
            } else if neg_mos {
                f.write_char('+')?;
            }
            write!(f, "{} day", days)?;
            if days != 1 {
                f.write_char('s')?;
            }
        }

        let non_zero_hmsn = hours > 0 || minutes > 0 || secs > 0 || nanos > 0;

        if (years == 0 && months == 0 && days == 0) || non_zero_hmsn {
            if years > 0 || months > 0 || days > 0 {
                f.write_char(' ')?;
            }
            if !self.is_positive_dur && non_zero_hmsn {
                f.write_char('-')?;
            } else if neg_mos {
                f.write_char('+')?;
            }
            write!(f, "{:02}:{:02}:{:02}", hours, minutes, secs)?;
            if nanos > 0 {
                let mut width = 9;
                while nanos % 10 == 0 {
                    width -= 1;
                    nanos /= 10;
                }
                write!(f, ".{:0width$}", nanos, width = width)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn interval_fmt() {
        fn mon(mon: i64) -> String {
            Interval {
                months: mon,
                ..Default::default()
            }
            .to_string()
        }

        assert_eq!(mon(1), "1 month");
        assert_eq!(mon(12), "1 year");
        assert_eq!(mon(13), "1 year 1 month");
        assert_eq!(mon(24), "2 years");
        assert_eq!(mon(25), "2 years 1 month");
        assert_eq!(mon(26), "2 years 2 months");

        fn dur(is_positive_dur: bool, d: u64) -> String {
            Interval {
                months: 0,
                duration: std::time::Duration::from_secs(d),
                is_positive_dur,
            }
            .to_string()
        }
        assert_eq!(&dur(true, 86_400 * 2), "2 days");
        assert_eq!(&dur(true, 86_400 * 2 + 3_600 * 3), "2 days 03:00:00");
        assert_eq!(
            &dur(true, 86_400 * 2 + 3_600 * 3 + 60 * 45 + 6),
            "2 days 03:45:06"
        );
        assert_eq!(
            &dur(true, 86_400 * 2 + 3_600 * 3 + 60 * 45),
            "2 days 03:45:00"
        );
        assert_eq!(&dur(true, 86_400 * 2 + 6), "2 days 00:00:06");
        assert_eq!(&dur(true, 86_400 * 2 + 60 * 45 + 6), "2 days 00:45:06");
        assert_eq!(&dur(true, 86_400 * 2 + 3_600 * 3 + 6), "2 days 03:00:06");
        assert_eq!(&dur(true, 3_600 * 3 + 60 * 45 + 6), "03:45:06");
        assert_eq!(&dur(true, 3_600 * 3 + 6), "03:00:06");
        assert_eq!(&dur(true, 3_600 * 3), "03:00:00");
        assert_eq!(&dur(true, 60 * 45 + 6), "00:45:06");
        assert_eq!(&dur(true, 60 * 45), "00:45:00");
        assert_eq!(&dur(true, 6), "00:00:06");

        assert_eq!(&dur(false, 86_400 * 2 + 6), "-2 days -00:00:06");
        assert_eq!(&dur(false, 86_400 * 2 + 60 * 45 + 6), "-2 days -00:45:06");
        assert_eq!(&dur(false, 86_400 * 2 + 3_600 * 3 + 6), "-2 days -03:00:06");
        assert_eq!(&dur(false, 3_600 * 3 + 60 * 45 + 6), "-03:45:06");
        assert_eq!(&dur(false, 3_600 * 3 + 6), "-03:00:06");
        assert_eq!(&dur(false, 3_600 * 3), "-03:00:00");
        assert_eq!(&dur(false, 60 * 45 + 6), "-00:45:06");
        assert_eq!(&dur(false, 60 * 45), "-00:45:00");
        assert_eq!(&dur(false, 6), "-00:00:06");

        fn mon_dur(mon: i64, is_positive_dur: bool, d: u64) -> String {
            Interval {
                months: mon,
                duration: std::time::Duration::from_secs(d),
                is_positive_dur,
            }
            .to_string()
        }
        assert_eq!(&mon_dur(1, true, 86_400 * 2 + 6), "1 month 2 days 00:00:06");
        assert_eq!(
            &mon_dur(1, true, 86_400 * 2 + 60 * 45 + 6),
            "1 month 2 days 00:45:06"
        );
        assert_eq!(
            &mon_dur(1, true, 86_400 * 2 + 3_600 * 3 + 6),
            "1 month 2 days 03:00:06"
        );
        assert_eq!(
            &mon_dur(26, true, 3_600 * 3 + 60 * 45 + 6),
            "2 years 2 months 03:45:06"
        );
        assert_eq!(
            &mon_dur(26, true, 3_600 * 3 + 6),
            "2 years 2 months 03:00:06"
        );
        assert_eq!(&mon_dur(26, true, 3_600 * 3), "2 years 2 months 03:00:00");
        assert_eq!(&mon_dur(26, true, 60 * 45 + 6), "2 years 2 months 00:45:06");
        assert_eq!(&mon_dur(26, true, 60 * 45), "2 years 2 months 00:45:00");
        assert_eq!(&mon_dur(26, true, 6), "2 years 2 months 00:00:06");

        assert_eq!(
            &mon_dur(26, false, 86_400 * 2 + 6),
            "2 years 2 months -2 days -00:00:06"
        );
        assert_eq!(
            &mon_dur(26, false, 86_400 * 2 + 60 * 45 + 6),
            "2 years 2 months -2 days -00:45:06"
        );
        assert_eq!(
            &mon_dur(26, false, 86_400 * 2 + 3_600 * 3 + 6),
            "2 years 2 months -2 days -03:00:06"
        );
        assert_eq!(
            &mon_dur(26, false, 3_600 * 3 + 60 * 45 + 6),
            "2 years 2 months -03:45:06"
        );
        assert_eq!(
            &mon_dur(26, false, 3_600 * 3 + 6),
            "2 years 2 months -03:00:06"
        );
        assert_eq!(&mon_dur(26, false, 3_600 * 3), "2 years 2 months -03:00:00");
        assert_eq!(
            &mon_dur(26, false, 60 * 45 + 6),
            "2 years 2 months -00:45:06"
        );
        assert_eq!(&mon_dur(26, false, 60 * 45), "2 years 2 months -00:45:00");
        assert_eq!(&mon_dur(26, false, 6), "2 years 2 months -00:00:06");

        assert_eq!(
            &mon_dur(-1, true, 86_400 * 2 + 6),
            "-1 month +2 days +00:00:06"
        );
        assert_eq!(
            &mon_dur(-1, true, 86_400 * 2 + 60 * 45 + 6),
            "-1 month +2 days +00:45:06"
        );
        assert_eq!(
            &mon_dur(-1, true, 86_400 * 2 + 3_600 * 3 + 6),
            "-1 month +2 days +03:00:06"
        );
        assert_eq!(
            &mon_dur(-26, true, 3_600 * 3 + 60 * 45 + 6),
            "-2 years -2 months +03:45:06"
        );
        assert_eq!(
            &mon_dur(-26, true, 3_600 * 3 + 6),
            "-2 years -2 months +03:00:06"
        );
        assert_eq!(
            &mon_dur(-26, true, 3_600 * 3),
            "-2 years -2 months +03:00:00"
        );
        assert_eq!(
            &mon_dur(-26, true, 60 * 45 + 6),
            "-2 years -2 months +00:45:06"
        );
        assert_eq!(&mon_dur(-26, true, 60 * 45), "-2 years -2 months +00:45:00");
        assert_eq!(&mon_dur(-26, true, 6), "-2 years -2 months +00:00:06");

        assert_eq!(
            &mon_dur(-26, false, 86_400 * 2 + 6),
            "-2 years -2 months -2 days -00:00:06"
        );
        assert_eq!(
            &mon_dur(-26, false, 86_400 * 2 + 60 * 45 + 6),
            "-2 years -2 months -2 days -00:45:06"
        );
        assert_eq!(
            &mon_dur(-26, false, 86_400 * 2 + 3_600 * 3 + 6),
            "-2 years -2 months -2 days -03:00:06"
        );
        assert_eq!(
            &mon_dur(-26, false, 3_600 * 3 + 60 * 45 + 6),
            "-2 years -2 months -03:45:06"
        );
        assert_eq!(
            &mon_dur(-26, false, 3_600 * 3 + 6),
            "-2 years -2 months -03:00:06"
        );
        assert_eq!(
            &mon_dur(-26, false, 3_600 * 3),
            "-2 years -2 months -03:00:00"
        );
        assert_eq!(
            &mon_dur(-26, false, 60 * 45 + 6),
            "-2 years -2 months -00:45:06"
        );
        assert_eq!(
            &mon_dur(-26, false, 60 * 45),
            "-2 years -2 months -00:45:00"
        );
        assert_eq!(&mon_dur(-26, false, 6), "-2 years -2 months -00:00:06");
    }

    #[test]
    fn test_interval_value_truncate_low_fields() {
        use DateTimeField::*;

        let mut test_cases = [
            (Year, None, (321, 654_321, 321_000_000), (26 * 12, 0, 0)),
            (Month, None, (321, 654_321, 321_000_000), (321, 0, 0)),
            (
                Day,
                None,
                (321, 654_321, 321_000_000),
                (321, 7 * 60 * 60 * 24, 0), // months: 321, duration: 604800s, is_positive_dur: true
            ),
            (
                Hour,
                None,
                (321, 654_321, 321_000_000),
                (321, 181 * 60 * 60, 0),
            ),
            (
                Minute,
                None,
                (321, 654_321, 321_000_000),
                (321, 10905 * 60, 0),
            ),
            (
                Second,
                None,
                (321, 654_321, 321_000_000),
                (321, 654_321, 321_000_000),
            ),
            (
                Second,
                Some(1),
                (321, 654_321, 321_000_000),
                (321, 654_321, 300_000_000),
            ),
            (
                Second,
                Some(0),
                (321, 654_321, 321_000_000),
                (321, 654_321, 0),
            ),
        ];

        for test in test_cases.iter_mut() {
            let mut i = Interval {
                months: (test.2).0,
                duration: std::time::Duration::new((test.2).1, (test.2).2),
                is_positive_dur: true,
            };
            let j = Interval {
                months: (test.3).0,
                duration: std::time::Duration::new((test.3).1, (test.3).2),
                is_positive_dur: true,
            };

            i.truncate_low_fields(test.0, test.1).unwrap();

            if i != j {
                panic!(
                "test_interval_value_truncate_low_fields failed on {} \n actual: {:?} \n expected: {:?}",
                test.0, i, j
            );
            }
        }
    }

    #[test]
    fn test_interval_value_truncate_high_fields() {
        use DateTimeField::*;

        let mut test_cases = [
            (Year, (321, 654_321), (321, 654_321)),
            (Month, (321, 654_321), (9, 654_321)),
            (Day, (321, 654_321), (0, 654_321)),
            (Hour, (321, 654_321), (0, 654_321 % (60 * 60 * 24))),
            (Minute, (321, 654_321), (0, 654_321 % (60 * 60))),
            (Second, (321, 654_321), (0, 654_321 % 60)),
        ];

        for test in test_cases.iter_mut() {
            let mut i = Interval {
                months: (test.1).0,
                duration: std::time::Duration::new((test.1).1 as u64, 123),
                is_positive_dur: true,
            };
            let j = Interval {
                months: (test.2).0,
                duration: std::time::Duration::new((test.2).1 as u64, 123),
                is_positive_dur: true,
            };

            i.truncate_high_fields(test.0);

            if i != j {
                panic!(
                "test_interval_value_truncate_high_fields failed on {} \n actual: {:?} \n expected: {:?}",
                test.0, i, j
            );
            }
        }
    }
}
