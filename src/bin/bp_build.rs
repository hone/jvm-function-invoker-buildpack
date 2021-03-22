use anyhow::anyhow;
use libcnb::{
    build::{cnb_runtime_build, GenericBuildContext},
    data,
    platform::Platform,
};
use serde::Deserialize;
use sha2::Digest;
use std::{
    fmt::Display,
    fs,
    io::{self, Write},
    process::Command,
};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

#[derive(Deserialize)]
struct FunctionBundleToml {
    function: Function,
}

#[derive(Deserialize)]
struct Function {
    class: String,
    payload_class: String,
    payload_media_type: String,
    return_class: String,
    return_media_type: String,
}

fn main() -> anyhow::Result<()> {
    cnb_runtime_build(build);

    Ok(())
}

fn build(ctx: GenericBuildContext) -> anyhow::Result<()> {
    let heroku_debug = ctx.platform.env().var("HEROKU_BUILDPACK_DEBUG").is_ok();
    header("Installing Java function runtime")?;

    let mut runtime_layer = ctx.layer("sf-fx-runtime-java")?;
    let buildpack_toml: data::buildpack::BuildpackToml = toml::from_str(&fs::read_to_string(
        ctx.buildpack_dir.join("buildpack.toml"),
    )?)?;

    let buildpack_metadata_runtime = buildpack_toml
        .metadata
        .get("runtime")
        .ok_or_else(|| anyhow!("buildpack.toml does not have `metadata.runtime` key"))?;
    let buildpack_sha256 = buildpack_metadata_runtime
        .get("sha256")
        .ok_or_else(|| anyhow!("buildpack.toml does not have `metadata.runtime.sha256` key"))?;
    let empty_string = toml::Value::String("".to_string());
    let runtime_layer_sha256 = runtime_layer
        .content_metadata()
        .metadata
        .get("runtime_jar_sha256")
        .unwrap_or(&empty_string);
    let runtime_jar_path = runtime_layer.as_path().join("runtime.jar");
    let runtime_jar_str = runtime_jar_path
        .to_str()
        .ok_or_else(|| anyhow!("runtime jar path is not a UTF-8 string"))?;
    let mut function_bundle_layer = ctx.layer("function-bundle")?;
    let function_bundle_layer_string = function_bundle_layer
        .as_path()
        .to_str()
        .ok_or_else(|| anyhow!("function bundle layer is not a UTF-8 string"))?
        .to_owned();

    if buildpack_sha256 == runtime_layer_sha256 && runtime_jar_path.exists() {
        info("Installed Java function runtime from cache")?;
    } else {
        debug("Creating function runtime layer", heroku_debug)?;
        let mut content_metadata = runtime_layer.mut_content_metadata();
        content_metadata.launch = true;
        content_metadata.build = false;
        content_metadata.cache = true;

        let runtime_url = buildpack_metadata_runtime
            .get("url")
            .ok_or_else(|| anyhow!("buildpack.toml does not have `metadata.runtime.url` key"))?;
        content_metadata
            .metadata
            .insert("runtime_jar_url".to_owned(), runtime_url.clone());
        // SHA256 checksum checking is disabled for as the function runtime is very unstable and is updated very often.
        // We don't want to trigger a whole release cycle just for a minor update. This code must be reactivated for beta/GA!
        //content_metadata
        //    .metadata
        //    .insert("runtime_jar_sha256".to_owned(), buildpack_sha256.clone());
        runtime_layer.write_content_metadata()?;

        debug("Function runtime layer successfully created", heroku_debug)?;

        info("Starting download of function runtime")?;
        let runtime_url_str = runtime_url
            .as_str()
            .ok_or_else(|| anyhow!("buildpack.toml's `metadata.runtime.url` is not a string"))?;
        download(runtime_url_str,
            &runtime_jar_path,
        ).map_err(|_| {
	  error("Download of function runtime failed", format!(r#"
We couldn't download the function runtime at {}.

This is usually caused by intermittent network issues. Please try again and contact us should the error persist.
"#, runtime_url)).unwrap_err()
        })?;
        info("Function runtime download successful")?;

        //        if buildpack_sha256 != &toml::Value::String(sha256(&fs::read(&runtime_jar_path)?)) {
        //            error(
        //                "Function runtime integrity check failed",
        //                r#"
        //We could not verify the integrity of the downloaded function runtime.
        //Please try again and contact us should the error persist.
        //"#,
        //            )?;
        //        }

        info("Function runtime installation successful")?;

        header("Detecting function")?;

        let mut content_metadata = function_bundle_layer.mut_content_metadata();
        content_metadata.launch = true;
        content_metadata.build = false;
        content_metadata.cache = false;
        function_bundle_layer.write_content_metadata()?;

        let exit_status = Command::new("java")
            .args(&[
                "-jar",
                runtime_jar_str,
                "bundle",
                ctx.app_dir
                    .to_str()
                    .ok_or_else(|| anyhow!("app dir is not a UTF-8 string"))?,
                &function_bundle_layer_string,
            ])
            .spawn()?
            .wait()?;

        if let Some(code) = exit_status.code() {
            match code {
                0 => {
                    info("Detection successful")?;
                    Ok(())
                }
                1 => error(
                    "No functions found",
                    r#"
Your project does not seem to contain any Java functions.
The output above might contain information about issues with your function.
"#,
                ),
                2 => error(
                    "Multiple functions found",
                    r#"
Your project contains multiple Java functions.
Currently, only projects that contain exactly one (1) function are supported.
"#,
                ),
                3..=6 => error(
                    "Detection failed",
                    format!(
                        r#"Function detection failed with internal error "{}""#,
                        code
                    ),
                ),
                _ => error(
                    "Detection failed",
                    format!(
                        r#"
Function detection failed with unexpected error code {}.
The output above might contain hints what caused this error to happen.
"#,
                        code
                    ),
                ),
            }?;
        }

        let function_bundle_toml: FunctionBundleToml = toml::from_slice(&fs::read(
            &function_bundle_layer.as_path().join("function-bundle.toml"),
        )?)?;

        header(format!(
            "Detected function: {}",
            function_bundle_toml.function.class
        ))?;
        info(format!(
            "Payload type: {}",
            function_bundle_toml.function.payload_class
        ))?;
        info(format!(
            "Return type: {}",
            function_bundle_toml.function.return_class
        ))?;
    }

    let mut launch = data::launch::Launch::new();
    let cmd = format!(
        "java -jar {} serve {} -p \\${{PORT:-8080}}",
        runtime_jar_str, &function_bundle_layer_string
    );
    launch.processes.push(data::launch::Process::new(
        "web",
        cmd,
        &[] as &[String],
        false,
    )?);

    Ok(())
}

fn header(msg: impl Display) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Magenta)).set_bold(true))?;
    writeln!(&mut stdout, "\n[{}]", msg)?;
    stdout.reset()?;

    Ok(())
}

fn info(msg: impl Display) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    stdout.reset()?;
    writeln!(&mut stdout, "[INFO] {}", msg)?;

    Ok(())
}

fn error(header: impl Display, msg: impl Display) -> anyhow::Result<()> {
    let mut stderr = StandardStream::stderr(ColorChoice::Always);
    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
    writeln!(&mut stderr, "\n[ERROR: {}]", header)?;
    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
    writeln!(&mut stderr, "{}", msg)?;
    stderr.reset()?;

    Err(anyhow!(format!("{}", header)))
}

fn debug(msg: impl Display, debug: bool) -> anyhow::Result<()> {
    if debug {
        let mut stdout = StandardStream::stdout(ColorChoice::Always);
        stdout.reset()?;
        writeln!(&mut stdout, "[DEBUG] {}", msg)?;
    }

    Ok(())
}

fn warning(header: impl Display, msg: impl Display) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)).set_bold(true))?;
    writeln!(&mut stdout, "\n[WARNING: {}]", header)?;
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
    writeln!(&mut stdout, "{}", msg)?;
    stdout.reset();

    Ok(())
}

fn download(uri: impl AsRef<str>, dst: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    let response = reqwest::blocking::get(uri.as_ref())?;
    let mut content = io::Cursor::new(response.bytes()?);
    let mut file = fs::File::create(dst.as_ref())?;
    io::copy(&mut content, &mut file)?;

    Ok(())
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(data))
}
