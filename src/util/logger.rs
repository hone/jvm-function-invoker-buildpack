use anyhow::anyhow;
use std::{fmt::Display, io::Write};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

pub fn header(msg: impl Display) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Magenta)).set_bold(true))?;
    writeln!(&mut stdout, "\n[{}]", msg)?;
    stdout.reset()?;

    Ok(())
}

pub fn info(msg: impl Display) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    stdout.reset()?;
    writeln!(&mut stdout, "[INFO] {}", msg)?;

    Ok(())
}

pub fn error(header: impl Display, msg: impl Display) -> anyhow::Result<()> {
    let mut stderr = StandardStream::stderr(ColorChoice::Always);
    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
    writeln!(&mut stderr, "\n[ERROR: {}]", header)?;
    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
    writeln!(&mut stderr, "{}", msg)?;
    stderr.reset()?;

    Err(anyhow!(format!("{}", header)))
}

pub fn debug(msg: impl Display, debug: bool) -> anyhow::Result<()> {
    if debug {
        let mut stdout = StandardStream::stdout(ColorChoice::Always);
        stdout.reset()?;
        writeln!(&mut stdout, "[DEBUG] {}", msg)?;
    }

    Ok(())
}

pub fn warning(header: impl Display, msg: impl Display) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)).set_bold(true))?;
    writeln!(&mut stdout, "\n[WARNING: {}]", header)?;
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
    writeln!(&mut stdout, "{}", msg)?;
    stdout.reset()?;

    Ok(())
}
