use colored::Colorize;

const PITBULL_HEAD: &str = r##"    /(
   //\\
  //   )_.-"""-._,-""-.
  \\ ^,'_\     /_\     )
   `./ /O\|   |/O\\   /
    \ \_/|   |\_/ \_/
     \ .'  _  `. /
      ( .:(_):. )
       `._.-._,'
         `-)"##;

const MOMO_TEXT: &str = r#"   __  __  __  __
  |  \/  ||  \/  |
  | .  . || .  . |
  | |\/| || |\/| |
  | |  | || |  | |
  |_|  |_||_|  |_|"#;

/// Print the MoMo startup banner.
///
/// Shows a pitbull head (MoMo) with "MOMO" text art and tagline.
/// Respects `NO_COLOR` environment variable.
pub fn print_banner() {
    if std::env::var("NO_COLOR").is_err() {
        println!(
            "{}\n{}\n{}",
            PITBULL_HEAD.bright_white(),
            MOMO_TEXT.bright_cyan().bold(),
            "  your AI coding companion".dimmed(),
        );
    } else {
        println!(
            "{}\n{}\n  your AI coding companion",
            PITBULL_HEAD, MOMO_TEXT,
        );
    }
    println!();
}
