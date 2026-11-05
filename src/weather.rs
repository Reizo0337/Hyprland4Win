//! Tiempo para el widget: Open-Meteo (gratuito y sin clave). La ciudad que escribe el usuario se envía al
//! servicio de geocodificación de Open-Meteo y no se comparte nada más (ni la IP se usa para localizar).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::widget_draw::{weekday_of, Day, Weather};

static STATE: Mutex<Option<Weather>> = Mutex::new(None);
/// (ciudad pedida, instante de la última descarga correcta, descarga en curso)
static FETCH: Mutex<(String, Option<Instant>, bool)> = Mutex::new((String::new(), None, false));

const REFRESH: Duration = Duration::from_secs(30 * 60);
const SHORT_DAYS: [&str; 7] = ["Lun", "Mar", "Mié", "Jue", "Vie", "Sáb", "Dom"];

pub fn latest() -> Option<Weather> {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Porcentaje-codifica un texto para ponerlo en una URL.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// (latitud, longitud, nombre) de la primera coincidencia de la geocodificación.
pub fn parse_geocode(json: &str) -> Option<(f64, f64, String)> {
    let v: Value = serde_json::from_str(json).ok()?;
    let r = v.get("results")?.get(0)?;
    Some((r.get("latitude")?.as_f64()?, r.get("longitude")?.as_f64()?, r.get("name")?.as_str()?.to_string()))
}

/// "2026-10-29" -> "Jue".
fn day_label(date: &str) -> Option<String> {
    let mut it = date.split('-').map(|p| p.parse::<i32>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    (1..=12).contains(&m).then(|| SHORT_DAYS[weekday_of(y, m as u32, d as u32) as usize].to_string())
}

/// Previsión de Open-Meteo: tiempo actual y los 3 días siguientes.
pub fn parse_forecast(json: &str, city: &str) -> Option<Weather> {
    let v: Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current")?;
    let temp = cur.get("temperature_2m")?.as_f64()?.round() as i32;
    let code = cur.get("weather_code")?.as_u64()? as u32;
    let daily = v.get("daily")?;
    let arr = |k: &str| daily.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let (dates, codes, hi, lo) = (arr("time"), arr("weather_code"), arr("temperature_2m_max"), arr("temperature_2m_min"));
    let days = (1..dates.len().min(4))
        .filter_map(|i| {
            Some(Day {
                label: day_label(dates.get(i)?.as_str()?)?,
                hi: hi.get(i)?.as_f64()?.round() as i32,
                lo: lo.get(i)?.as_f64()?.round() as i32,
                code: codes.get(i)?.as_u64()? as u32,
            })
        })
        .collect();
    Some(Weather { city: city.to_string(), temp, code, days })
}

fn get(url: &str) -> Option<String> {
    crate::net::http_get_text(url)
}

/// Descarga el tiempo de `city` (bloquea; úsalo en un hilo).
pub fn fetch(city: &str) -> Option<Weather> {
    let geo = get(&format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=es&format=json", url_encode(city)))?;
    let (lat, lon, name) = parse_geocode(&geo)?;
    let forecast = get(&format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=4"
    ))?;
    parse_forecast(&forecast, &name)
}

/// Pide el tiempo de `city` en segundo plano si no hay datos recientes de esa ciudad.
/// `on_ready` se llama desde el hilo cuando llegan datos nuevos.
pub fn refresh(city: &str, on_ready: fn()) {
    let city = city.trim().to_string();
    if city.is_empty() {
        return;
    }
    {
        let mut f = FETCH.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = f.0 == city && f.1.is_some_and(|t| t.elapsed() < REFRESH);
        if f.2 || fresh {
            return;
        }
        if f.0 != city {
            *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        f.0 = city.clone();
        f.2 = true;
    }
    let _ = std::thread::Builder::new().name("hypr-win-weather".into()).spawn(move || {
        let result = fetch(&city);
        let mut f = FETCH.lock().unwrap_or_else(|e| e.into_inner());
        f.2 = false;
        if let Some(w) = result {
            f.1 = Some(Instant::now());
            drop(f);
            *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
            on_ready();
        } else {
            // sin red o ciudad desconocida: se reintenta dentro de unos minutos
            f.1 = Some(Instant::now() - REFRESH + Duration::from_secs(5 * 60));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_encoded() {
        assert_eq!(url_encode("Madrid"), "Madrid");
        assert_eq!(url_encode("San José"), "San%20Jos%C3%A9");
        assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn geocoding_response_is_parsed() {
        let j = r#"{"results":[{"id":1,"name":"Madrid","latitude":40.4165,"longitude":-3.7026,"country":"España"}]}"#;
        let (lat, lon, name) = parse_geocode(j).unwrap();
        assert!((lat - 40.4165).abs() < 1e-6 && (lon + 3.7026).abs() < 1e-6 && name == "Madrid");
        assert!(parse_geocode(r#"{"generationtime_ms":0.5}"#).is_none()); // ciudad desconocida
        assert!(parse_geocode("no es json").is_none());
    }

    #[test]
    fn forecast_response_is_parsed() {
        let j = r#"{"current":{"temperature_2m":21.4,"weather_code":2},
            "daily":{"time":["2026-10-29","2026-10-30","2026-10-31","2026-11-01"],
                     "weather_code":[2,0,63,3],
                     "temperature_2m_max":[22.1,23.0,19.4,17.2],
                     "temperature_2m_min":[12.0,12.6,11.5,9.1]}}"#;
        let w = parse_forecast(j, "Madrid").unwrap();
        assert_eq!((w.temp, w.code, w.city.as_str()), (21, 2, "Madrid"));
        assert_eq!(w.days.len(), 3);
        assert_eq!(w.days[0], Day { label: "Vie".into(), hi: 23, lo: 13, code: 0 });
        assert_eq!(w.days[1].label, "Sáb");
        assert_eq!(w.days[2], Day { label: "Dom".into(), hi: 17, lo: 9, code: 3 });
        assert!(parse_forecast(r#"{"current":{}}"#, "x").is_none());
    }
}
