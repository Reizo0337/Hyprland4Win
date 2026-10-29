//! Lecturas reales (solo lectura) de audio, Wi-Fi y batería, más la lógica pura asociada.

use hypr_win::wifi::{self, Auth};
use hypr_win::{audio, power};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

#[test]
fn reads_audio_wifi_and_battery() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let vol = audio::get();
    println!("volumen: {vol:?}, dispositivo detectado: {}", audio::device_name().is_some());
    if let Some(v) = vol {
        assert!((0.0..=1.0).contains(&v.level));
    }

    // sin imprimir nombres de redes: solo contadores
    let st = wifi::status();
    println!("wifi: presente={} radio={} conectado={} nombre_visible={} denegado={}", st.present, st.enabled, st.connected, st.ssid.is_some(), st.denied);
    let scan = wifi::networks();
    let nets = scan.networks;
    println!("{} redes visibles ({} guardadas), acceso denegado: {}", nets.len(), nets.iter().filter(|n| n.known).count(), scan.denied);
    assert!(nets.iter().all(|n| n.signal <= 100 && !n.ssid.is_empty()));

    let bt = hypr_win::bluetooth::status();
    println!("bluetooth: presente={} encendido={} conectados={}", bt.present, bt.on, bt.connected.len());

    println!("batería: {:?}", power::battery());
}

#[test]
fn wifi_profile_xml() {
    let x = wifi::profile_xml("Mi <Red> & \"Co\"", Auth::Psk, "p&ss<word>").unwrap();
    assert!(x.contains("<name>Mi &lt;Red&gt; &amp; &quot;Co&quot;</name>"));
    assert!(x.contains("WPA2PSK") && x.contains("<keyMaterial>p&amp;ss&lt;word&gt;</keyMaterial>"));
    let open = wifi::profile_xml("Cafe", Auth::Open, "").unwrap();
    assert!(open.contains("<authentication>open</authentication>") && !open.contains("sharedKey"));
    assert!(wifi::profile_xml("Corp", Auth::Other, "x").is_none());
    assert!(wifi::profile_xml("Nuevo", Auth::Sae, "x").unwrap().contains("WPA3SAE"));
}

#[test]
fn auth_classification() {
    assert_eq!(wifi::auth_from(1, false), Auth::Open);
    assert_eq!(wifi::auth_from(7, true), Auth::Psk);
    assert_eq!(wifi::auth_from(9, true), Auth::Sae);
    assert_eq!(wifi::auth_from(6, true), Auth::Other); // WPA2 empresa
    assert_eq!(wifi::auth_from(1, true), Auth::Other); // WEP
}
