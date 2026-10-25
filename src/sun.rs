//! Lever et coucher du soleil, calculés sur place (équation du lever de soleil, précision
//! de l'ordre de la minute). Aucune connexion Internet nécessaire.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

const J2000: f64 = 2_451_545.0;
const UNIX_EPOCH_JD: f64 = 2_440_587.5;

fn sin_d(x: f64) -> f64 {
    x.to_radians().sin()
}

fn cos_d(x: f64) -> f64 {
    x.to_radians().cos()
}

fn jd_to_utc(jd: f64) -> Option<DateTime<Utc>> {
    let secs = (jd - UNIX_EPOCH_JD) * 86_400.0;
    Utc.timestamp_opt(secs.round() as i64, 0).single()
}

/// Lever et coucher du soleil (UTC) pour une date et un lieu.
/// `None` pendant le jour ou la nuit polaire.
pub fn sun_times(
    date: NaiveDate,
    latitude: f64,
    longitude: f64,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let days = (date - NaiveDate::from_ymd_opt(1970, 1, 1)?).num_days() as f64;
    // Midi solaire approximatif du jour, en jours depuis J2000.
    let n = (days + UNIX_EPOCH_JD + 0.5 - J2000 + 0.0008).round();
    let mean_solar = n - longitude / 360.0;
    let m = (357.5291 + 0.985_600_28 * mean_solar).rem_euclid(360.0);
    let c = 1.9148 * sin_d(m) + 0.02 * sin_d(2.0 * m) + 0.0003 * sin_d(3.0 * m);
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let transit = J2000 + mean_solar + 0.0053 * sin_d(m) - 0.0069 * sin_d(2.0 * lambda);
    let sin_decl = sin_d(lambda) * sin_d(23.4397);
    let cos_decl = sin_decl.asin().cos();
    let cos_hour = (sin_d(-0.833) - sin_d(latitude) * sin_decl) / (cos_d(latitude) * cos_decl);
    if !(-1.0..=1.0).contains(&cos_hour) {
        return None;
    }
    let hour = cos_hour.acos().to_degrees();
    Some((
        jd_to_utc(transit - hour / 360.0)?,
        jd_to_utc(transit + hour / 360.0)?,
    ))
}

/// Fait-il nuit à cet instant ? (avant le lever ou après le coucher du jour même)
pub fn is_dark(now: DateTime<Utc>, latitude: f64, longitude: f64) -> bool {
    match sun_times(now.date_naive(), latitude, longitude) {
        Some((rise, set)) => now < rise || now > set,
        // Jour ou nuit polaire : on tranche par la saison.
        None => {
            let summer =
                (4..=9).contains(&now.format("%m").to_string().parse::<u32>().unwrap_or(1));
            (latitude > 0.0) != summer
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn minutes(t: DateTime<Utc>) -> i64 {
        (t.hour() * 60 + t.minute()) as i64
    }

    #[test]
    fn paris_summer_solstice() {
        // Paris, 21 juin : lever vers 03:47 UTC, coucher vers 19:58 UTC.
        let (rise, set) = sun_times(
            NaiveDate::from_ymd_opt(2026, 6, 21).unwrap(),
            48.8566,
            2.3522,
        )
        .unwrap();
        assert!((minutes(rise) - (3 * 60 + 47)).abs() <= 4, "lever {rise}");
        assert!((minutes(set) - (19 * 60 + 58)).abs() <= 4, "coucher {set}");
    }

    #[test]
    fn nantes_winter() {
        // Nantes, 21 décembre : lever vers 07:52 UTC, coucher vers 16:17 UTC (Paris 15:56 + 16 min de longitude + jour un peu plus long).
        let (rise, set) = sun_times(
            NaiveDate::from_ymd_opt(2026, 12, 21).unwrap(),
            47.2184,
            -1.5536,
        )
        .unwrap();
        assert!((minutes(rise) - (7 * 60 + 52)).abs() <= 5, "lever {rise}");
        assert!((minutes(set) - (16 * 60 + 17)).abs() <= 5, "coucher {set}");
    }

    #[test]
    fn polar_night() {
        assert!(sun_times(NaiveDate::from_ymd_opt(2026, 12, 21).unwrap(), 78.2, 15.6).is_none());
        let noon = Utc.with_ymd_and_hms(2026, 12, 21, 12, 0, 0).unwrap();
        assert!(is_dark(noon, 78.2, 15.6));
    }
}
