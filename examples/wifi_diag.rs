//! Diagnóstico de la API WLAN: códigos de retorno por adaptador (sin imprimir nombres de redes).

use std::ptr::null_mut;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    wlan_intf_opcode_current_connection, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList,
    WlanOpenHandle, WlanQueryInterface, WLAN_INTERFACE_INFO_LIST,
};

fn main() {
    unsafe {
        let mut negotiated = 0u32;
        let mut h = HANDLE::default();
        println!("WlanOpenHandle = {}", WlanOpenHandle(2, None, &mut negotiated, &mut h));
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        println!("WlanEnumInterfaces = {}", WlanEnumInterfaces(h, None, &mut list));
        if list.is_null() {
            return;
        }
        let l = &*list;
        println!("adaptadores: {}", l.dwNumberOfItems);
        for i in 0..l.dwNumberOfItems as usize {
            let info = &*l.InterfaceInfo.as_ptr().add(i);
            let end = info.strInterfaceDescription.iter().position(|&c| c == 0).unwrap_or(0);
            let desc = String::from_utf16_lossy(&info.strInterfaceDescription[..end]);
            println!("[{i}] {desc}  estado={}", info.isState.0);

            let (mut size, mut data) = (0u32, null_mut());
            let r = WlanQueryInterface(h, &info.InterfaceGuid, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None);
            println!("    conexión actual: código {r}");
            if r == 0 {
                WlanFreeMemory(data);
            }

            let mut nets = null_mut();
            let r = WlanGetAvailableNetworkList(h, &info.InterfaceGuid, 0, None, &mut nets);
            let count = if r == 0 && !nets.is_null() { (*nets).dwNumberOfItems } else { 0 };
            println!("    redes disponibles: código {r}, {count} redes");
        }
        WlanFreeMemory(list as *const _);
        WlanCloseHandle(h, None);
    }
}
