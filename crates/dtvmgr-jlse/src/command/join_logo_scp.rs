//! Wrapper for the `join_logo_scp` external command.
//!
//! Merges logo frame detection and chapter information to produce CM
//! cut points.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::Result;

use crate::types::DetectionParam;

/// Run `join_logo_scp` with the given inputs and detection parameters.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
pub fn run(
    binary: &Path,
    logoframe_txt: &Path,
    chapterexe_txt: &Path,
    jl_command_file: &Path,
    output_avs_cut: &Path,
    jlscp_output: &Path,
    param: &DetectionParam,
) -> Result<()> {
    let args = build_args(
        logoframe_txt,
        chapterexe_txt,
        jl_command_file,
        output_avs_cut,
        jlscp_output,
        param,
    );
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run(binary, &os_args)
}

/// Run `join_logo_scp` with stderr captured via `on_log` callback.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
#[allow(clippy::too_many_arguments)]
pub fn run_logged(
    binary: &Path,
    logoframe_txt: &Path,
    chapterexe_txt: &Path,
    jl_command_file: &Path,
    output_avs_cut: &Path,
    jlscp_output: &Path,
    param: &DetectionParam,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    let args = build_args(
        logoframe_txt,
        chapterexe_txt,
        jl_command_file,
        output_avs_cut,
        jlscp_output,
        param,
    );
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run_logged(binary, &os_args, on_log)
}

/// Build the argument list for `join_logo_scp`.
///
/// The `-flags` argument is omitted when `param.flags` is empty.
/// `param.options` is split on whitespace and appended as individual
/// arguments.
#[must_use]
pub fn build_args(
    logoframe_txt: &Path,
    chapterexe_txt: &Path,
    jl_command_file: &Path,
    output_avs_cut: &Path,
    jlscp_output: &Path,
    param: &DetectionParam,
) -> Vec<String> {
    let mut args = vec![
        "-inlogo".to_owned(),
        logoframe_txt.display().to_string(),
        "-inscp".to_owned(),
        chapterexe_txt.display().to_string(),
        "-incmd".to_owned(),
        jl_command_file.display().to_string(),
        "-o".to_owned(),
        output_avs_cut.display().to_string(),
        "-oscp".to_owned(),
        jlscp_output.display().to_string(),
    ];

    if !param.flags.is_empty() {
        args.push("-flags".to_owned());
        args.push(param.flags.clone());
    }

    for opt in param.options.split_whitespace() {
        args.push(opt.to_owned());
    }

    args
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::path::Path;

    use super::*;

    fn base_args_len() -> usize {
        // -inlogo val -inscp val -incmd val -o val -oscp val = 10
        10
    }

    #[test]
    fn test_build_args_with_flags_and_options() {
        // Arrange
        let param = DetectionParam {
            jl_run: "JL_NHK.txt".to_owned(),
            flags: "fLOff,fHCWOWA".to_owned(),
            options: "-MaxFadeIn 30 -MaxFadeOut 30".to_owned(),
        };

        // Act
        let args = build_args(
            Path::new("/out/obs_logoframe.txt"),
            Path::new("/out/obs_chapterexe.txt"),
            Path::new("/jl/JL_NHK.txt"),
            Path::new("/out/obs_cut.avs"),
            Path::new("/out/obs_jlscp.txt"),
            &param,
        );

        // Assert
        assert_eq!(args[0], "-inlogo");
        assert_eq!(args[1], "/out/obs_logoframe.txt");
        assert_eq!(args[2], "-inscp");
        assert_eq!(args[3], "/out/obs_chapterexe.txt");
        assert_eq!(args[4], "-incmd");
        assert_eq!(args[5], "/jl/JL_NHK.txt");
        assert_eq!(args[6], "-o");
        assert_eq!(args[7], "/out/obs_cut.avs");
        assert_eq!(args[8], "-oscp");
        assert_eq!(args[9], "/out/obs_jlscp.txt");
        assert_eq!(args[10], "-flags");
        assert_eq!(args[11], "fLOff,fHCWOWA");
        assert_eq!(args[12], "-MaxFadeIn");
        assert_eq!(args[13], "30");
        assert_eq!(args[14], "-MaxFadeOut");
        assert_eq!(args[15], "30");
        assert_eq!(args.len(), 16);
    }

    #[test]
    fn test_build_args_empty_flags() {
        // Arrange
        let param = DetectionParam {
            jl_run: "JL.txt".to_owned(),
            flags: String::new(),
            options: String::new(),
        };

        // Act
        let args = build_args(
            Path::new("/a"),
            Path::new("/b"),
            Path::new("/c"),
            Path::new("/d"),
            Path::new("/e"),
            &param,
        );

        // Assert — no -flags, no options
        assert_eq!(args.len(), base_args_len());
        assert!(!args.contains(&"-flags".to_owned()));
    }

    #[test]
    fn test_build_args_flags_no_options() {
        // Arrange
        let param = DetectionParam {
            jl_run: "JL.txt".to_owned(),
            flags: "fLOff".to_owned(),
            options: String::new(),
        };

        // Act
        let args = build_args(
            Path::new("/a"),
            Path::new("/b"),
            Path::new("/c"),
            Path::new("/d"),
            Path::new("/e"),
            &param,
        );

        // Assert
        assert_eq!(args.len(), base_args_len() + 2); // -flags + value
        assert_eq!(args[10], "-flags");
        assert_eq!(args[11], "fLOff");
    }

    #[test]
    fn test_build_args_options_whitespace_splitting() {
        // Arrange
        let param = DetectionParam {
            jl_run: "JL.txt".to_owned(),
            flags: String::new(),
            options: " -a 1  -b 2 ".to_owned(),
        };

        // Act
        let args = build_args(
            Path::new("/a"),
            Path::new("/b"),
            Path::new("/c"),
            Path::new("/d"),
            Path::new("/e"),
            &param,
        );

        // Assert — leading/trailing/multiple spaces handled
        assert_eq!(args.len(), base_args_len() + 4);
        assert_eq!(args[10], "-a");
        assert_eq!(args[11], "1");
        assert_eq!(args[12], "-b");
        assert_eq!(args[13], "2");
    }
}
