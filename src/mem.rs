//! Memoria: devuelve a Windows las páginas que el proceso ya no necesita.
//!
//! Tras cerrar un panel o cada cierto tiempo en reposo se recorta el conjunto de trabajo; lo que haga falta
//! de nuevo vuelve solo (fallos de página suaves), así que el único coste es un instante de CPU.

use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

pub fn trim() {
    unsafe {
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}
