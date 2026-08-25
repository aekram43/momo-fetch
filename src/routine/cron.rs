//! Five-field cron expressions, evaluated in an IANA timezone.
//!
//! `min hour dom month dow` — the shape every crontab(5) on the machine already
//! uses, and the shape the routine form asks for. Deliberately not the six- or
//! seven-field variants: a seconds column invites schedules that fire faster
//! than a turn can finish, and the scheduler ticks in tens of seconds anyway.
//!
//! Two rules that are easy to get wrong and hard to notice:
//!
//! - **`dom` and `dow` are OR'd when both are restricted.** `0 0 1 * mon` is
//!   "the 1st *or* any Monday", not "a Monday that is the 1st". That is what
//!   crontab does, and a scheduler that quietly disagrees with the user's
//!   muscle memory is worse than one that has no cron at all.
//! - **The next occurrence is computed in the routine's own zone, then mapped
//!   back to UTC.** Storing a UTC-only schedule means "09:00 daily" drifts by
//!   an hour twice a year for everyone who does not live in UTC.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

/// How far ahead [`CronExpr::next_naive_after`] will look before giving up.
///
/// Bounded because an expression like `0 0 30 2 *` (February 30th) matches
/// nothing, ever, and an unbounded search would spin forever on it.
const SEARCH_LIMIT_DAYS: i64 = 366 * 5;

/// A parsed five-field cron expression.
///
/// Each field is a bitmask over its own domain, so matching is a shift and a
/// test rather than a re-parse per candidate minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    minute: u64,
    hour: u64,
    dom: u64,
    month: u64,
    dow: u64,
    /// `*` in the day-of-month column — needed for the OR rule above.
    dom_star: bool,
    /// `*` in the day-of-week column.
    dow_star: bool,
    source: String,
}

const MONTH_NAMES: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DOW_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

impl CronExpr {
    /// Parse `min hour dom month dow`.
    ///
    /// Accepts `*`, `n`, `a-b`, `*/n`, `a-b/n`, comma lists, and the usual
    /// three-letter month and weekday names. `7` in the weekday column means
    /// Sunday, as it does in crontab.
    pub fn parse(expr: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "Cron expression needs exactly five fields (min hour dom month dow), got {}: '{}'",
                fields.len(),
                expr.trim()
            ));
        }

        let (minute, _) = parse_field(fields[0], 0, 59, &[], "minute")?;
        let (hour, _) = parse_field(fields[1], 0, 23, &[], "hour")?;
        let (dom, dom_star) = parse_field(fields[2], 1, 31, &[], "day-of-month")?;
        let (month, _) = parse_field(fields[3], 1, 12, &MONTH_NAMES, "month")?;
        let (dow, dow_star) = parse_field(fields[4], 0, 7, &DOW_NAMES, "day-of-week")?;

        // 7 and 0 are both Sunday. Fold before matching so `sun`, `0` and `7`
        // are one bit rather than three near-misses.
        let dow = if dow & (1 << 7) != 0 {
            (dow & !(1 << 7)) | 1
        } else {
            dow
        };

        Ok(Self {
            minute,
            hour,
            dom,
            month,
            dow,
            dom_star,
            dow_star,
            source: expr.split_whitespace().collect::<Vec<_>>().join(" "),
        })
    }

    /// The normalised expression — whitespace collapsed, nothing else changed.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// The next firing at or after `after`, in `tz`, as a UTC instant.
    ///
    /// Strictly *after*: a schedule that already fired at this minute must not
    /// be handed the same minute again, or a tick loop would re-fire it every
    /// pass for a whole minute.
    pub fn next_after(&self, after: DateTime<Utc>, tz: Tz) -> Option<DateTime<Utc>> {
        let mut candidate = self.next_naive_after(after.with_timezone(&tz).naive_local())?;
        // A spring-forward gap means the wall clock time the user asked for did
        // not happen that day. Skip to the next match rather than inventing an
        // instant; `earliest()` also settles the autumn ambiguity by firing on
        // the first pass through the repeated hour, not the second.
        for _ in 0..SEARCH_LIMIT_DAYS {
            if let Some(dt) = tz.from_local_datetime(&candidate).earliest() {
                return Some(dt.with_timezone(&Utc));
            }
            candidate = self.next_naive_after(candidate)?;
        }
        None
    }

    /// The next matching wall-clock minute strictly after `from`.
    ///
    /// Skips by the largest unit that cannot match — a `0 3 1 * *` expression
    /// jumps month to month instead of walking half a million minutes.
    fn next_naive_after(&self, from: NaiveDateTime) -> Option<NaiveDateTime> {
        let mut t = (from + Duration::minutes(1))
            .with_second(0)?
            .with_nanosecond(0)?;
        let limit = from + Duration::days(SEARCH_LIMIT_DAYS);

        while t <= limit {
            if !self.matches_month(t.month()) {
                t = start_of_next_month(t)?;
                continue;
            }
            if !self.matches_day(t.date()) {
                t = start_of_next_day(t)?;
                continue;
            }
            if !self.matches_hour(t.hour()) {
                t = start_of_next_hour(t)?;
                continue;
            }
            if !self.matches_minute(t.minute()) {
                t += Duration::minutes(1);
                continue;
            }
            return Some(t);
        }
        None
    }

    fn matches_minute(&self, m: u32) -> bool {
        self.minute & (1 << m) != 0
    }

    fn matches_hour(&self, h: u32) -> bool {
        self.hour & (1 << h) != 0
    }

    fn matches_month(&self, m: u32) -> bool {
        self.month & (1 << m) != 0
    }

    /// crontab's day rule: restrict on both columns and they are OR'd.
    fn matches_day(&self, date: NaiveDate) -> bool {
        let dom_hit = self.dom & (1 << date.day()) != 0;
        let dow_hit = self.dow & (1 << date.weekday().num_days_from_sunday()) != 0;
        match (self.dom_star, self.dow_star) {
            (true, true) => true,
            (true, false) => dow_hit,
            (false, true) => dom_hit,
            (false, false) => dom_hit || dow_hit,
        }
    }
}

fn start_of_next_day(t: NaiveDateTime) -> Option<NaiveDateTime> {
    t.date().succ_opt()?.and_hms_opt(0, 0, 0)
}

fn start_of_next_hour(t: NaiveDateTime) -> Option<NaiveDateTime> {
    Some((t + Duration::hours(1)).with_minute(0)?.with_second(0)?)
}

fn start_of_next_month(t: NaiveDateTime) -> Option<NaiveDateTime> {
    let (y, m) = if t.month() == 12 {
        (t.year() + 1, 1)
    } else {
        (t.year(), t.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1)?.and_hms_opt(0, 0, 0)
}

/// Parse one column into a bitmask, reporting whether it was a bare `*`.
fn parse_field(
    spec: &str,
    min: u64,
    max: u64,
    names: &[&str],
    label: &str,
) -> Result<(u64, bool), String> {
    if spec.is_empty() {
        return Err(format!("Empty {label} field."));
    }
    let star = spec == "*";
    let mut mask = 0u64;

    for item in spec.split(',') {
        let (range_part, step) = match item.split_once('/') {
            Some((r, s)) => {
                let step: u64 = s
                    .parse()
                    .map_err(|_| format!("Bad step '{s}' in {label} field '{spec}'."))?;
                if step == 0 {
                    return Err(format!("Step must be at least 1 in {label} field '{spec}'."));
                }
                (r, step)
            }
            None => (item, 1),
        };

        let (lo, hi) = if range_part == "*" {
            (min, max)
        } else if let Some((a, b)) = range_part.split_once('-') {
            (
                parse_value(a, min, max, names, label)?,
                parse_value(b, min, max, names, label)?,
            )
        } else {
            let v = parse_value(range_part, min, max, names, label)?;
            // `5/15` means "from 5, every 15" — a bare value with a step is an
            // open-ended range, not a single point.
            if step > 1 { (v, max) } else { (v, v) }
        };

        if lo > hi {
            return Err(format!(
                "Range {lo}-{hi} runs backwards in {label} field '{spec}'."
            ));
        }
        let mut v = lo;
        while v <= hi {
            mask |= 1 << v;
            v += step;
        }
    }

    Ok((mask, star))
}

fn parse_value(raw: &str, min: u64, max: u64, names: &[&str], label: &str) -> Result<u64, String> {
    let token = raw.trim().to_ascii_lowercase();
    if let Some(idx) = names.iter().position(|n| *n == token) {
        // Month names start at 1, weekday names at 0 — `min` carries which.
        return Ok(idx as u64 + min);
    }
    let value: u64 = token
        .parse()
        .map_err(|_| format!("'{raw}' is not a valid {label} value."))?;
    if value < min || value > max {
        return Err(format!(
            "{label} value {value} is outside {min}-{max} (in '{raw}')."
        ));
    }
    Ok(value)
}

/// Resolve an IANA zone name, with a message that names the field.
pub fn parse_timezone(name: &str) -> Result<Tz, String> {
    name.parse::<Tz>()
        .map_err(|_| format!("'{name}' is not an IANA timezone (try UTC, or Asia/Bangkok)."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn next(expr: &str, from: &str, tz: &str) -> String {
        CronExpr::parse(expr)
            .unwrap()
            .next_after(utc(from), parse_timezone(tz).unwrap())
            .unwrap()
            .to_rfc3339()
    }

    #[test]
    fn hourly_lands_on_the_hour() {
        assert_eq!(
            next("0 * * * *", "2026-08-25T10:17:04Z", "UTC"),
            "2026-08-25T11:00:00+00:00"
        );
    }

    #[test]
    fn next_is_strictly_after() {
        // Exactly on a firing minute: the answer is the *following* one, or a
        // tick loop re-fires the same window for sixty seconds.
        assert_eq!(
            next("0 * * * *", "2026-08-25T10:00:00Z", "UTC"),
            "2026-08-25T11:00:00+00:00"
        );
    }

    #[test]
    fn step_and_list() {
        assert_eq!(
            next("*/15 * * * *", "2026-08-25T10:01:00Z", "UTC"),
            "2026-08-25T10:15:00+00:00"
        );
        assert_eq!(
            next("5,35 * * * *", "2026-08-25T10:10:00Z", "UTC"),
            "2026-08-25T10:35:00+00:00"
        );
    }

    #[test]
    fn a_bare_value_with_a_step_is_open_ended() {
        assert_eq!(
            next("5/20 * * * *", "2026-08-25T10:06:00Z", "UTC"),
            "2026-08-25T10:25:00+00:00"
        );
    }

    #[test]
    fn timezone_is_the_routines_own() {
        // 09:00 in Bangkok is 02:00 UTC — the whole reason the zone is stored.
        // (The answer comes back as the UTC instant it maps to.)
        assert_eq!(
            next("0 9 * * *", "2026-08-25T00:00:00Z", "Asia/Bangkok"),
            "2026-08-25T02:00:00+00:00"
        );
    }

    #[test]
    fn named_month_and_weekday() {
        assert_eq!(
            next("0 0 * jan mon", "2026-08-25T00:00:00Z", "UTC"),
            "2027-01-04T00:00:00+00:00"
        );
    }

    #[test]
    fn seven_and_zero_are_both_sunday() {
        let a = CronExpr::parse("0 0 * * 0").unwrap();
        let b = CronExpr::parse("0 0 * * 7").unwrap();
        let from = utc("2026-08-25T00:00:00Z");
        let tz = parse_timezone("UTC").unwrap();
        assert_eq!(a.next_after(from, tz), b.next_after(from, tz));
    }

    #[test]
    fn dom_and_dow_are_ored_when_both_restricted() {
        // 2026-09-01 is a Tuesday; the rule must also catch the Mondays.
        let expr = CronExpr::parse("0 0 1 * mon").unwrap();
        let tz = parse_timezone("UTC").unwrap();
        let first = expr.next_after(utc("2026-08-30T12:00:00Z"), tz).unwrap();
        assert_eq!(first.to_rfc3339(), "2026-08-31T00:00:00+00:00"); // a Monday
        let second = expr.next_after(first, tz).unwrap();
        assert_eq!(second.to_rfc3339(), "2026-09-01T00:00:00+00:00"); // the 1st
    }

    #[test]
    fn dst_spring_forward_skips_the_hour_that_did_not_happen() {
        // US Eastern jumps 02:00 → 03:00 on 2026-03-08, so a 02:30 daily job
        // has no instant that day and must land on the 9th instead.
        assert_eq!(
            next("30 2 * * *", "2026-03-07T12:00:00Z", "America/New_York"),
            "2026-03-09T06:30:00+00:00"
        );
    }

    #[test]
    fn impossible_expressions_terminate() {
        let expr = CronExpr::parse("0 0 30 2 *").unwrap();
        assert!(
            expr.next_after(utc("2026-08-25T00:00:00Z"), parse_timezone("UTC").unwrap())
                .is_none()
        );
    }

    #[test]
    fn field_count_is_reported_plainly() {
        let err = CronExpr::parse("0 * * * * *").unwrap_err();
        assert!(err.contains("five fields"), "{err}");
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        assert!(CronExpr::parse("60 * * * *").is_err());
        assert!(CronExpr::parse("0 24 * * *").is_err());
        assert!(CronExpr::parse("0 0 0 * *").is_err());
        assert!(CronExpr::parse("0 0 * 13 *").is_err());
        assert!(CronExpr::parse("0 0 * * 8").is_err());
        assert!(CronExpr::parse("*/0 * * * *").is_err());
    }

    #[test]
    fn unknown_timezone_names_the_field() {
        let err = parse_timezone("Mars/Olympus").unwrap_err();
        assert!(err.contains("IANA"), "{err}");
    }
}
