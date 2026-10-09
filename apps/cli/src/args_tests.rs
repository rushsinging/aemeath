use super::*;

#[test]
fn cli_accepts_connect_subcommand_without_run() {
    let cli = Cli::try_parse_from(["aemeath", "connect"]).unwrap();
    assert!(matches!(cli.command, Some(Commands::Connect)));
}

#[test]
fn tui_and_quiet_modes_map_native_stderr_ownership() {
    let tui = Cli::try_parse_from(["aemeath"]).unwrap();
    let quiet = Cli::try_parse_from(["aemeath", "--quiet"]).unwrap();
    let tui_verbose = Cli::try_parse_from(["aemeath", "--verbose"]).unwrap();
    let quiet_verbose = Cli::try_parse_from(["aemeath", "--quiet", "--verbose"]).unwrap();

    assert_eq!(
        sdk::ChatBootstrapArgs::from(Args::from(tui.run_args)).native_stderr,
        sdk::NativeStderrMode::RouteToLogs
    );
    assert_eq!(
        sdk::ChatBootstrapArgs::from(Args::from(tui_verbose.run_args)).native_stderr,
        sdk::NativeStderrMode::RouteToLogs
    );
    assert_eq!(
        sdk::ChatBootstrapArgs::from(Args::from(quiet.run_args)).native_stderr,
        sdk::NativeStderrMode::Preserve
    );
    assert_eq!(
        sdk::ChatBootstrapArgs::from(Args::from(quiet_verbose.run_args)).native_stderr,
        sdk::NativeStderrMode::Preserve
    );
}

#[test]
fn yolo_and_allow_all_alias_project_same_runtime_bootstrap_acl() {
    let yolo = Cli::try_parse_from(["aemeath", "--yolo"]).unwrap();
    let alias = Cli::try_parse_from(["aemeath", "--allow-all"]).unwrap();

    let yolo_bootstrap = sdk::ChatBootstrapArgs::from(Args::from(yolo.run_args));
    let alias_bootstrap = sdk::ChatBootstrapArgs::from(Args::from(alias.run_args));

    assert!(yolo_bootstrap.allow_all);
    assert!(alias_bootstrap.allow_all);
}

#[test]
fn test_cli_rejects_provider_argument() {
    assert!(Cli::try_parse_from(["aemeath", "--provider", "Zhipu"]).is_err());
}

#[test]
fn test_cli_accepts_model_selection() {
    let cli = Cli::try_parse_from(["aemeath", "--model", "Zhipu/glm-5.1"]).unwrap();

    assert_eq!(cli.run_args.model.as_deref(), Some("Zhipu/glm-5.1"));
}

#[test]
fn test_args_from_run_args_has_no_provider_field_requirement() {
    let cli =
        Cli::try_parse_from(["aemeath", "--model", "LiteLLM/anthropic/claude-opus-4-7"]).unwrap();
    let args = Args::from(cli.run_args);

    assert_eq!(
        args.model.as_deref(),
        Some("LiteLLM/anthropic/claude-opus-4-7")
    );
}

#[test]
fn test_cli_accepts_quiet_short_flag() {
    let cli = Cli::try_parse_from(["aemeath", "-q"]).unwrap();

    assert!(cli.run_args.quiet);
}

#[test]
fn test_cli_accepts_quiet_long_flag() {
    let cli = Cli::try_parse_from(["aemeath", "--quiet"]).unwrap();

    assert!(cli.run_args.quiet);
}

#[test]
fn test_cli_accepts_verbose_short_flag() {
    let cli = Cli::try_parse_from(["aemeath", "-v"]).unwrap();

    assert!(cli.run_args.verbose);
}

#[test]
fn default_cli_maps_to_file_logging_output() {
    let cli = Cli::try_parse_from(["aemeath"]).unwrap();
    let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

    assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::File);
}

#[test]
fn verbose_cli_maps_to_stderr_logging_output() {
    // 仅在 no-tui（--quiet）下 --verbose 走 stderr，保留实时日志语义。
    let cli = Cli::try_parse_from(["aemeath", "--quiet", "--verbose"]).unwrap();
    let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

    assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::Stderr);
}

#[test]
fn tui_verbose_stays_file_to_avoid_stderr_polluting_alternate_screen() {
    // TUI 模式（非 --quiet）下 --verbose 绝不走 stderr——stderr 会越过
    // alternate screen 的双缓冲直接糊屏（#1215）。
    let cli = Cli::try_parse_from(["aemeath", "--verbose"]).unwrap();
    let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

    assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::File);
}

#[test]
fn quiet_cli_maps_to_file_logging_output() {
    let cli = Cli::try_parse_from(["aemeath", "--quiet"]).unwrap();
    let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

    assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::File);
}

#[test]
fn verbose_logging_output_takes_precedence_over_quiet() {
    let cli = Cli::try_parse_from(["aemeath", "--quiet", "--verbose"]).unwrap();
    let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

    assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::Stderr);
}

#[test]
fn test_args_from_run_args_carries_quiet_flag() {
    let cli = Cli::try_parse_from(["aemeath", "--quiet"]).unwrap();
    let args = Args::from(cli.run_args);

    assert!(args.quiet);
}

#[test]
fn cli_accepts_systemone_download_nested_command() {
    let cli = Cli::try_parse_from(["aemeath", "systemone", "download"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Commands::Systemone {
            command: SystemoneCommands::Download
        })
    ));
}

#[test]
fn cli_rejects_systemone_without_subcommand() {
    let result = Cli::try_parse_from(["aemeath", "systemone"]);
    assert!(result.is_err(), "`aemeath systemone` 缺少子命令应解析失败");
}
