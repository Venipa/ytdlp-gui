//! Custom argument lines (Settings, Custom arguments): one option per line, split like a shell
//! would. yt-dlp lines are checked by the engine with yt-dlp's own parser; aria2c lines only for
//! their shape here, since asking aria2c would mean starting a download.
use crate::ArgCheck;

/// One line's text split like a shell would; `None` on an unbalanced quote.
pub fn split(line: &str) -> Option<Vec<String>> {
    shlex::split(line)
}

/// Arguments grouped one option per line (`--a 1 --b` is two lines), each joined back with the
/// quotes it needs. A value before any option stays on its own line, so the check flags it.
pub fn lines(args: &[String]) -> Vec<String> {
    let option = |a: &String| a.starts_with('-') && a.parse::<f64>().is_err();
    let mut groups: Vec<Vec<&str>> = vec![];
    for a in args {
        match groups.last_mut() {
            Some(g) if !option(a) => g.push(a),
            _ => groups.push(vec![a]),
        }
    }
    groups
        .into_iter()
        .map(|g| shlex::try_join(g).unwrap_or_default())
        .collect()
}

/// aria2c options yt-dlp or ytdlpd pass; the progress reading needs `--auto-save-interval`.
const ARIA2C_OWN: [&str; 13] = [
    "-d",
    "--dir",
    "-o",
    "--out",
    "-i",
    "--input-file",
    "--auto-save-interval",
    "--allow-overwrite",
    "--auto-file-renaming",
    "--always-resume",
    "--force-save",
    "--remove-control-file",
    "--enable-rpc",
];

/// The shape of one aria2c line: it starts with an option and sets none aria2c gets from us.
pub fn check_aria2c(args: &[String]) -> ArgCheck {
    let Some(first) = args.first() else {
        return ArgCheck::Ok;
    };
    if !first.starts_with('-') {
        return ArgCheck::Invalid {
            message: format!("not an option: {first}"),
            suggestion: None,
        };
    }
    let own = args
        .iter()
        .filter(|a| a.starts_with('-'))
        .any(|a| ARIA2C_OWN.contains(&a.split('=').next().unwrap_or(a)));
    if own {
        ArgCheck::Reserved
    } else {
        ArgCheck::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_pasted_line_becomes_one_option_per_line() {
        let args = split(r#"--sleep-interval 2 --user-agent "My Agent/1.0" --no-mtime"#).unwrap();
        assert_eq!(
            lines(&args),
            [
                "--sleep-interval 2",
                "--user-agent 'My Agent/1.0'",
                "--no-mtime"
            ]
        );
        // a negative number is a value, a leading word keeps its own line
        assert_eq!(
            lines(&v(&["hello", "--offset", "-5"])),
            ["hello", "--offset -5"]
        );
        assert_eq!(split(r#"--x "open"#), None);
    }

    #[test]
    fn aria2c_shape() {
        assert_eq!(check_aria2c(&v(&["--max-tries=5"])), ArgCheck::Ok);
        assert_eq!(check_aria2c(&v(&["-x", "8"])), ArgCheck::Ok);
        assert_eq!(
            check_aria2c(&v(&["--auto-save-interval=10"])),
            ArgCheck::Reserved
        );
        assert_eq!(check_aria2c(&v(&["-d", "C:/x"])), ArgCheck::Reserved);
        assert!(matches!(
            check_aria2c(&v(&["max-tries=5"])),
            ArgCheck::Invalid { .. }
        ));
        assert_eq!(check_aria2c(&[]), ArgCheck::Ok);
    }
}
