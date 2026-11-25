pub(crate) fn normalize(
    name: Option<String>,
    names: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let mut names = match (name, names) {
        (Some(_), Some(_)) => return Err("Specify selected_name or selected_names".into()),
        (Some(name), None) => vec![name],
        (None, names) => names.unwrap_or_default(),
    };
    if names.len() > 50 {
        return Err("Select at most 50 group/work names".into());
    }
    if names.iter().any(|name| name.is_empty() || name.len() > 512) {
        return Err("Selected group/work names must contain 1..=512 bytes each".into());
    }
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalizes_exact_names_and_preserves_single_name_calls() {
        let names = vec!["z".into(), " Group ".into(), "a".into(), "z".into()];
        assert_eq!(
            normalize(None, Some(names)).unwrap(),
            [" Group ", "a", "z"]
        );
        assert_eq!(normalize(Some("*".into()), None).unwrap(), ["*"]);
        assert!(normalize(None, Some(vec![])).unwrap().is_empty());
        assert!(normalize(None, None).unwrap().is_empty());
    }

    #[test]
    fn validates_raw_count_and_utf8_byte_lengths() {
        assert!(normalize(None, Some(vec!["a".into(); 51])).is_err());
        assert!(normalize(None, Some(vec!["a".into(); 50])).is_ok());
        assert!(normalize(None, Some(vec![String::new()])).is_err());
        assert!(normalize(None, Some(vec!["я".repeat(256)])).is_ok());
        assert!(normalize(None, Some(vec!["я".repeat(257)])).is_err());
        assert!(normalize(Some("a".into()), Some(vec!["b".into()])).is_err());
    }
}
