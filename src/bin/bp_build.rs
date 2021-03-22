use anyhow::anyhow;
use jvm_function_invoker_buildpack::{
    function_bundle,
    util::{self, logger::*},
};
use libcnb::{
    build::{cnb_runtime_build, GenericBuildContext},
    data,
    platform::Platform,
};
use std::{fs, process::Command};

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
    let mut function_bundle_layer = ctx.layer("function-bundle")?;

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
        util::download(runtime_url_str,
            &runtime_jar_path,
        ).map_err(|_| {
	  error("Download of function runtime failed", format!(r#"
We couldn't download the function runtime at {}.

This is usually caused by intermittent network issues. Please try again and contact us should the error persist.
"#, runtime_url)).unwrap_err()
        })?;
        info("Function runtime download successful")?;

        //        if buildpack_sha256 != &toml::Value::String(util::sha256(&fs::read(&runtime_jar_path)?)) {
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
            .arg("-jar")
            .arg(&runtime_jar_path)
            .arg("bundle")
            .arg(&ctx.app_dir)
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

        let function_bundle_toml: function_bundle::Toml = toml::from_slice(&fs::read(
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
        runtime_jar_path.display(),
        function_bundle_layer.as_path().display(),
    );
    launch.processes.push(data::launch::Process::new(
        "web",
        cmd,
        &[] as &[String],
        false,
    )?);

    Ok(())
}
