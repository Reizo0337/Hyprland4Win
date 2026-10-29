//! Búsqueda difusa de nombres de aplicaciones (lógica pura).

/// Puntúa `name` frente a `query`; `None` si no coincide. Mayor es mejor.
/// Orden de preferencia: igual > prefijo > inicio de palabra > subcadena > letras en orden.
pub fn score(query: &str, name: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let n = name.to_lowercase();
    let len_penalty = (n.chars().count() as i32).min(99);
    if n == q {
        return Some(1000);
    }
    if n.starts_with(&q) {
        return Some(900 - len_penalty);
    }
    if n.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && w.starts_with(&q)) {
        return Some(700 - len_penalty);
    }
    if let Some(pos) = n.find(&q) {
        return Some(500 - (pos as i32).min(200) - len_penalty);
    }
    // letras de la consulta en orden (p. ej. "vsc" -> "Visual Studio Code")
    let mut chars = n.chars();
    let mut gaps = 0;
    for qc in q.chars().filter(|c| !c.is_whitespace()) {
        loop {
            match chars.next() {
                Some(c) if c == qc => break,
                Some(_) => gaps += 1,
                None => return None,
            }
        }
    }
    Some(300 - gaps.min(200))
}

/// Índices de `names` que coinciden, mejores primero (empates en el orden original).
pub fn rank(query: &str, names: &[String], limit: usize) -> Vec<usize> {
    let mut v: Vec<(i32, usize)> =
        names.iter().enumerate().filter_map(|(i, n)| score(query, n).map(|s| (s, i))).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.into_iter().take(limit).map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ["Calculadora", "Visual Studio Code", "Windows Terminal", "Terminal de Git", "Configuración", "Chrome"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn empty_query_keeps_order() {
        assert_eq!(rank("", &names(), 10), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn prefix_beats_word_start_and_substring() {
        // "Terminal de Git" (prefijo) antes que "Windows Terminal" (inicio de palabra)
        assert_eq!(rank("term", &names(), 10), vec![3, 2]);
        assert_eq!(rank("ome", &names(), 10), vec![5]);
    }

    #[test]
    fn subsequence_and_case_insensitive() {
        assert_eq!(rank("VSC", &names(), 10), vec![1]);
        assert!(rank("zzz", &names(), 10).is_empty());
    }

    #[test]
    fn limit_applies() {
        assert_eq!(rank("", &names(), 2).len(), 2);
    }
}
