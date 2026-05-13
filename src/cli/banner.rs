use colored::Colorize;

const PITBULL: &str = concat!(
    "\n",
    "                      /(\n",
    "                     //\\\\\n",
    "                    //   )_.-\"\"\"-._,-\"\"-.\n",
    "                    \\\\ ^,'_\\     /_\\     )\n",
    "                     `./ /O\\|   |/O\\\\   /\n",
    "                      \\ \\_/|   |\\_/ \\_/\n",
    "                       \\ .'  _  `. /\n",
    "                   .-.  ( .:(_):. )  ,-.\n",
    "                  (   `._`._.-._,'_,'   )\n",
    "                   )                   (\n",
    "                  (   .-------------.   )\n",
    "                   `-'               `-'",
);

const MOMO_TEXT: &str = concat!(
    "\n",
    "                                     __      _       _\n",
    "  _ __ ___   ___  _ __ ___   ___    / _| ___| |_ ___| |__\n",
    " | '_ ` _ \\ / _ \\| '_ ` _ \\ / _ \\  | |_ / _ \\ __/ __| '_ \\",
    "\n",
    " | | | | | | (_) | | | | | | (_) | |  _|  __/ || (__| | | |\n",
    " |_| |_| |_|\\___/|_| |_| |_|\\___/  |_|  \\___|\\__\\___|_| |_|\n",
);

const TAGLINE: &str = "  Play with MOMO, Let MOMO Fetch Your Perfect Match";

/// Print the MoMo startup banner.
///
/// Shows a pitbull ASCII art, MOMO text, and tagline.
/// Respects `NO_COLOR` environment variable.
pub fn print_banner() {
    if std::env::var("NO_COLOR").is_err() {
        println!(
            "{}\n{}\n{}",
            PITBULL.bright_white(),
            MOMO_TEXT.bright_cyan().bold(),
            TAGLINE.bright_cyan(),
        );
    } else {
        println!(
            "{}\n{}\n{}",
            PITBULL, MOMO_TEXT, TAGLINE,
        );
    }
    println!();
}
