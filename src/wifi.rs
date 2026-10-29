//! Wi-Fi con la API WLAN nativa (`wlanapi`, documentada): estado, redes, conexión y radio.

use std::ffi::c_void;
use std::ptr::null_mut;

use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    dot11_BSS_type_infrastructure, dot11_radio_state_off, dot11_radio_state_on, wlan_connection_mode_profile,
    wlan_intf_opcode_current_connection, wlan_intf_opcode_radio_state, WlanCloseHandle, WlanConnect, WlanDisconnect,
    WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanOpenHandle, WlanQueryInterface, WlanScan,
    WlanSetInterface, WlanSetProfile, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_ATTRIBUTES,
    WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO_LIST, WLAN_PHY_RADIO_STATE, WLAN_RADIO_STATE,
};

/// WLAN_AVAILABLE_NETWORK_CONNECTED
const NETWORK_CONNECTED: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    Open,
    /// WPA2-Personal
    Psk,
    /// WPA3-Personal
    Sae,
    /// 802.1X (empresa) u otros: no se pueden configurar desde aquí.
    Other,
}

#[derive(Clone, Debug)]
pub struct Network {
    pub ssid: String,
    /// 0-100
    pub signal: u32,
    pub secured: bool,
    pub connected: bool,
    /// Ya existe un perfil guardado en Windows (se conecta sin pedir contraseña).
    pub known: bool,
    pub auth: Auth,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Hay un adaptador Wi-Fi.
    pub present: bool,
    /// La radio está encendida.
    pub enabled: bool,
    pub ssid: Option<String>,
    pub signal: u32,
}

struct Client {
    handle: HANDLE,
    guid: GUID,
}

impl Client {
    fn open() -> Option<Client> {
        unsafe {
            let mut negotiated = 0u32;
            let mut handle = HANDLE::default();
            if WlanOpenHandle(2, None, &mut negotiated, &mut handle) != 0 {
                return None;
            }
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
            let found = if WlanEnumInterfaces(handle, None, &mut list) == 0 && !list.is_null() {
                let l = &*list;
                let g = (l.dwNumberOfItems > 0).then(|| l.InterfaceInfo[0].InterfaceGuid);
                WlanFreeMemory(list as *const c_void);
                g
            } else {
                None
            };
            match found {
                Some(guid) => Some(Client { handle, guid }),
                None => {
                    WlanCloseHandle(handle, None);
                    None
                }
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.handle, None);
        }
    }
}

fn ssid_string(len: u32, bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..(len as usize).min(bytes.len())]).into_owned()
}

pub fn status() -> Status {
    let Some(c) = Client::open() else { return Status::default() };
    let mut st = Status { present: true, enabled: radio_on(&c).unwrap_or(true), ..Default::default() };
    unsafe {
        let mut size = 0u32;
        let mut data: *mut c_void = null_mut();
        if WlanQueryInterface(c.handle, &c.guid, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None) == 0
            && !data.is_null()
        {
            let a = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
            let assoc = &a.wlanAssociationAttributes;
            st.ssid = Some(ssid_string(assoc.dot11Ssid.uSSIDLength, &assoc.dot11Ssid.ucSSID));
            st.signal = assoc.wlanSignalQuality;
            WlanFreeMemory(data);
        }
    }
    st
}

fn radio_on(c: &Client) -> Option<bool> {
    unsafe {
        let mut size = 0u32;
        let mut data: *mut c_void = null_mut();
        if WlanQueryInterface(c.handle, &c.guid, wlan_intf_opcode_radio_state, None, &mut size, &mut data, None) != 0
            || data.is_null()
        {
            return None;
        }
        let r = &*(data as *const WLAN_RADIO_STATE);
        let on = r.dwNumberOfPhys == 0 || r.PhyRadioState[0].dot11SoftwareRadioState == dot11_radio_state_on;
        WlanFreeMemory(data);
        Some(on)
    }
}

/// Redes visibles, una por SSID (la de mejor señal): la conectada primero, luego por señal.
pub fn networks() -> Vec<Network> {
    let Some(c) = Client::open() else { return Vec::new() };
    let mut out: Vec<Network> = Vec::new();
    unsafe {
        let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = null_mut();
        if WlanGetAvailableNetworkList(c.handle, &c.guid, 0, None, &mut list) != 0 || list.is_null() {
            return out;
        }
        let l = &*list;
        let items = std::slice::from_raw_parts(l.Network.as_ptr(), l.dwNumberOfItems as usize);
        for n in items {
            let ssid = ssid_string(n.dot11Ssid.uSSIDLength, &n.dot11Ssid.ucSSID);
            if ssid.is_empty() {
                continue; // red oculta
            }
            let net = Network {
                ssid,
                signal: n.wlanSignalQuality,
                secured: n.bSecurityEnabled.as_bool(),
                connected: n.dwFlags & NETWORK_CONNECTED != 0,
                known: n.strProfileName[0] != 0,
                auth: auth_from(n.dot11DefaultAuthAlgorithm.0, n.bSecurityEnabled.as_bool()),
            };
            match out.iter_mut().find(|o| o.ssid == net.ssid) {
                Some(o) if o.signal >= net.signal && !net.connected => {}
                Some(o) => *o = net,
                None => out.push(net),
            }
        }
        WlanFreeMemory(list as *const c_void);
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    out
}

/// DOT11_AUTH_ALGORITHM -> tipo de autenticación soportado.
pub fn auth_from(algo: i32, secured: bool) -> Auth {
    match algo {
        1 if !secured => Auth::Open,
        1 | 2 if secured => Auth::Other, // WEP: no se ofrece
        7 => Auth::Psk,
        4 => Auth::Psk, // WPA-Personal: se guarda como WPA2 y Windows negocia
        9 => Auth::Sae,
        10 => Auth::Open, // OWE
        _ => Auth::Other,
    }
}

/// Pide un nuevo escaneo (el resultado llega unos segundos después).
pub fn scan() {
    if let Some(c) = Client::open() {
        unsafe {
            WlanScan(c.handle, &c.guid, None, None, None);
        }
    }
}

pub fn set_radio(on: bool) -> Result<(), String> {
    let c = Client::open().ok_or("no hay adaptador Wi-Fi")?;
    let state = WLAN_PHY_RADIO_STATE {
        dwPhyIndex: 0,
        dot11SoftwareRadioState: if on { dot11_radio_state_on } else { dot11_radio_state_off },
        dot11HardwareRadioState: if on { dot11_radio_state_on } else { dot11_radio_state_off },
    };
    let r = unsafe {
        WlanSetInterface(
            c.handle,
            &c.guid,
            wlan_intf_opcode_radio_state,
            std::mem::size_of::<WLAN_PHY_RADIO_STATE>() as u32,
            &state as *const _ as *const c_void,
            None,
        )
    };
    if r == 0 { Ok(()) } else { Err(format!("Windows rechazó el cambio (código {r})")) }
}

pub fn disconnect() {
    if let Some(c) = Client::open() {
        unsafe {
            WlanDisconnect(c.handle, &c.guid, None);
        }
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Perfil WLAN (XML) para una red personal o abierta.
pub fn profile_xml(ssid: &str, auth: Auth, password: &str) -> Option<String> {
    let name = xml_escape(ssid);
    let (authentication, encryption, key) = match auth {
        Auth::Open => ("open", "none", String::new()),
        Auth::Psk => ("WPA2PSK", "AES", shared_key(password)),
        Auth::Sae => ("WPA3SAE", "AES", shared_key(password)),
        Auth::Other => return None,
    };
    Some(format!(
        "<?xml version=\"1.0\"?><WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\">\
<name>{name}</name><SSIDConfig><SSID><name>{name}</name></SSID></SSIDConfig>\
<connectionType>ESS</connectionType><connectionMode>auto</connectionMode>\
<MSM><security><authEncryption><authentication>{authentication}</authentication>\
<encryption>{encryption}</encryption><useOneX>false</useOneX></authEncryption>{key}</security></MSM></WLANProfile>"
    ))
}

fn shared_key(password: &str) -> String {
    format!(
        "<sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>",
        xml_escape(password)
    )
}

/// Conecta a `net`. Las redes con perfil guardado no necesitan contraseña; las nuevas se guardan
/// con `password` (o sin ella si son abiertas).
pub fn connect(net: &Network, password: Option<&str>) -> Result<(), String> {
    let c = Client::open().ok_or("no hay adaptador Wi-Fi")?;
    unsafe {
        if !net.known {
            let xml = profile_xml(&net.ssid, net.auth, password.unwrap_or(""))
                .ok_or("este tipo de red (empresa/WEP) no se puede configurar aquí")?;
            let xml = HSTRING::from(xml);
            let mut reason = 0u32;
            let r = WlanSetProfile(c.handle, &c.guid, 0, PCWSTR(xml.as_ptr()), PCWSTR::null(), true, None, &mut reason);
            if r != 0 {
                return Err(format!("no se pudo guardar el perfil (código {r})"));
            }
        }
        let profile = HSTRING::from(net.ssid.as_str());
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(profile.as_ptr()),
            pDot11Ssid: null_mut(),
            pDesiredBssidList: null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let r = WlanConnect(c.handle, &c.guid, &params, None);
        if r == 0 { Ok(()) } else { Err(format!("no se pudo conectar (código {r})")) }
    }
}
