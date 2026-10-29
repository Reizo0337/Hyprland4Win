//! Identidad de una aplicación en ejecución y su correspondencia con las entradas de `shell:AppsFolder`
//! (lógica pura).

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AppIdentity {
    /// AppUserModelID de la ventana (apps de la Tienda y muchas de escritorio).
    pub aumid: Option<String>,
    /// Ruta completa del ejecutable.
    pub exe: String,
}

impl AppIdentity {
    /// Nombre del ejecutable en minúsculas ("code.exe").
    pub fn exe_name(&self) -> String {
        self.exe.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
    }

    /// Clave para agrupar ventanas de la misma aplicación.
    pub fn key(&self) -> String {
        self.aumid.as_ref().map(|a| a.to_lowercase()).unwrap_or_else(|| self.exe.to_lowercase())
    }

    /// ¿Corresponde a la entrada `id` de `shell:AppsFolder`? Por AppUserModelID o por el nombre del
    /// ejecutable con el que acaba el identificador de las apps de escritorio.
    pub fn matches_entry(&self, id: &str) -> bool {
        let id = id.to_lowercase();
        if self.aumid.as_ref().is_some_and(|a| a.to_lowercase() == id) {
            return true;
        }
        let name = self.exe_name();
        // los alojados por ApplicationFrameHost se distinguen solo por AUMID
        !name.is_empty() && name != "applicationframehost.exe" && (id == name || id.ends_with(&format!("\\{name}")))
    }

    /// Nombre legible de respaldo cuando no hay entrada en el índice ("Code").
    pub fn fallback_name(&self) -> String {
        let stem = self.exe_name();
        let stem = stem.strip_suffix(".exe").unwrap_or(&stem).to_string();
        let mut c = stem.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => "App".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(aumid: Option<&str>, exe: &str) -> AppIdentity {
        AppIdentity { aumid: aumid.map(String::from), exe: exe.into() }
    }

    #[test]
    fn matches_by_aumid_or_exe_name() {
        let term = id(Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"), r"C:\Program Files\WindowsApps\x\WindowsTerminal.exe");
        assert!(term.matches_entry("microsoft.windowsterminal_8wekyb3d8bbwe!app"));
        let code = id(None, r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\Code.exe");
        assert!(code.matches_entry(r"{6D809377-6AF0-444B-8957-A3773F02200E}\Microsoft VS Code\Code.exe"));
        assert!(!code.matches_entry(r"{6D809377}\Microsoft VS Code\Decode.exe")); // no basta con terminar igual
        assert!(!code.matches_entry("Otra.App!App"));
    }

    #[test]
    fn frame_host_only_matches_by_aumid() {
        let calc = id(Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), r"C:\Windows\System32\ApplicationFrameHost.exe");
        assert!(calc.matches_entry("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(!calc.matches_entry(r"{1AC1}\System32\ApplicationFrameHost.exe"));
    }

    #[test]
    fn keys_and_names() {
        assert_eq!(id(Some("A.B"), r"C:\x.exe").key(), "a.b");
        assert_eq!(id(None, r"C:\Apps\Notepad.EXE").key(), r"c:\apps\notepad.exe");
        assert_eq!(id(None, r"C:\Apps\notepad.exe").fallback_name(), "Notepad");
    }
}
