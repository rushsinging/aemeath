use std::io::{self, IsTerminal, Write};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectEffect {
    Apply {
        session_id: sdk::ConnectSessionId,
        revision: sdk::ConnectRevision,
        command: sdk::ConnectCommand,
    },
    Cancel {
        session_id: sdk::ConnectSessionId,
        revision: sdk::ConnectRevision,
    },
    Refresh {
        session_id: sdk::ConnectSessionId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectInputMode {
    Visible,
    Masked,
}

pub(crate) fn input_mode_for_stage(stage: sdk::ConnectStage) -> ConnectInputMode {
    if stage == sdk::ConnectStage::EditCredential {
        ConnectInputMode::Masked
    } else {
        ConnectInputMode::Visible
    }
}

pub(crate) struct ConnectUiModel {
    view: sdk::ConnectView,
    input: String,
}

impl ConnectUiModel {
    pub(crate) fn new(view: sdk::ConnectView) -> Self {
        Self {
            view,
            input: String::new(),
        }
    }

    pub(crate) fn view(&self) -> &sdk::ConnectView {
        &self.view
    }

    pub(crate) fn replace_view(&mut self, view: sdk::ConnectView) {
        self.view = view;
        self.input.clear();
    }

    pub(crate) fn visible_input(&self) -> String {
        match input_mode_for_stage(self.view.stage) {
            ConnectInputMode::Visible => self.input.clone(),
            ConnectInputMode::Masked => "•".repeat(self.input.chars().count()),
        }
    }

    pub(crate) fn update(&mut self, key: crossterm::event::KeyEvent) -> Option<ConnectEffect> {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc => Some(ConnectEffect::Cancel {
                session_id: self.view.session_id.clone(),
                revision: self.view.revision,
            }),
            KeyCode::Backspace => {
                self.input.pop();
                None
            }
            KeyCode::Char(character) => {
                self.input.push(character);
                None
            }
            KeyCode::Enter => match command_for_input(&self.view, &self.input) {
                Ok(command) => Some(ConnectEffect::Apply {
                    session_id: self.view.session_id.clone(),
                    revision: self.view.revision,
                    command,
                }),
                Err(_) => None,
            },
            _ => None,
        }
    }
}

pub(crate) struct ConnectProjection {
    lines: Vec<String>,
}

impl ConnectProjection {
    pub(crate) fn from_view(view: &sdk::ConnectView) -> Self {
        let mut lines = vec![format!("Provider 配置 · {:?}", view.stage)];
        if view.stage == sdk::ConnectStage::SelectProvider {
            lines.push("内置 Provider:".to_string());
            lines.extend(
                view.catalog
                    .iter()
                    .map(|provider| format!("  {} ({})", provider.source, provider.driver)),
            );
        }
        lines.extend([
            format!(
                "Provider: {}",
                view.draft.source.as_deref().unwrap_or("未选择")
            ),
            format!(
                "Base URL: {}",
                view.draft.base_url.as_deref().unwrap_or("未设置")
            ),
            format!(
                "API Key: {}",
                if view.draft.has_api_key {
                    "••••••••"
                } else {
                    "未设置"
                }
            ),
        ]);
        if let Some(model) = &view.draft.model {
            lines.push(format!("Model: {}", model.model_id));
        }
        if let Some(status) = &view.probe_status {
            lines.push(format!("连接测试: {status:?}"));
        }
        if let Some(error) = &view.last_error {
            lines.push(format!("错误: {}", error.message));
        }
        Self { lines }
    }

    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }
}

pub(crate) fn command_for_input(
    view: &sdk::ConnectView,
    input: &str,
) -> Result<sdk::ConnectCommand, String> {
    let value = input.trim();
    match view.stage {
        sdk::ConnectStage::SelectProvider => Ok(sdk::ConnectCommand::SelectProvider {
            source: value.to_string(),
        }),
        sdk::ConnectStage::ConfirmOverwrite => match value.to_ascii_lowercase().as_str() {
            "y" | "yes" => Ok(sdk::ConnectCommand::ConfirmOverwrite),
            "n" | "no" => Ok(sdk::ConnectCommand::RejectOverwrite),
            _ => Err("请输入 y 或 n".to_string()),
        },
        sdk::ConnectStage::EditEndpoint => Ok(sdk::ConnectCommand::SetEndpoint {
            base_url: value.to_string(),
        }),
        sdk::ConnectStage::EditCredential => Ok(sdk::ConnectCommand::SetCredential {
            api_key: input.trim_end().to_string(),
        }),
        sdk::ConnectStage::EditUserAgent => Ok(sdk::ConnectCommand::SetProviderUserAgent {
            raw: (!value.is_empty()).then(|| value.to_string()),
        }),
        sdk::ConnectStage::SelectModel => parse_model_command(value),
        sdk::ConnectStage::EditCustomModel => parse_custom_model(value),
        sdk::ConnectStage::ChooseGlobalDefault => parse_yes_no(value)
            .map(|set_as_default| sdk::ConnectCommand::SetGlobalDefault { set_as_default }),
        sdk::ConnectStage::ChooseProbe => parse_yes_no(value).map(|probe| {
            if probe {
                sdk::ConnectCommand::BeginProbe
            } else {
                sdk::ConnectCommand::SkipProbe
            }
        }),
        sdk::ConnectStage::Review => match value.to_ascii_lowercase().as_str() {
            "y" | "yes" | "save" => Ok(sdk::ConnectCommand::ConfirmSave),
            "continue" => Ok(sdk::ConnectCommand::ContinueAfterProbe),
            "edit" => Ok(sdk::ConnectCommand::EditAfterProbeFailure),
            _ => Err("请输入 y、continue 或 edit".to_string()),
        },
        sdk::ConnectStage::Probing
        | sdk::ConnectStage::Saving
        | sdk::ConnectStage::Completed
        | sdk::ConnectStage::Cancelled => Err("当前阶段不接受输入".to_string()),
    }
}

fn parse_model_command(value: &str) -> Result<sdk::ConnectCommand, String> {
    if value.eq_ignore_ascii_case("custom") {
        return Ok(sdk::ConnectCommand::EnterCustomModel);
    }
    let index = value
        .parse::<usize>()
        .map_err(|_| "请输入推荐模型序号或 custom".to_string())?;
    let index = index
        .checked_sub(1)
        .ok_or_else(|| "模型序号从 1 开始".to_string())?;
    Ok(sdk::ConnectCommand::SelectRecommendedModel { index })
}

fn parse_custom_model(value: &str) -> Result<sdk::ConnectCommand, String> {
    let fields = value.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 {
        return Err("请输入：<model-id> <context-window> <max-tokens>".to_string());
    }
    Ok(sdk::ConnectCommand::SetCustomModel {
        model_id: fields[0].to_string(),
        context_window: fields[1]
            .parse()
            .map_err(|_| "context-window 必须是正整数".to_string())?,
        max_tokens: fields[2]
            .parse()
            .map_err(|_| "max-tokens 必须是正整数".to_string())?,
    })
}

fn parse_yes_no(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Err("请输入 y 或 n".to_string()),
    }
}

pub(crate) async fn execute_effect(
    connect: &dyn sdk::ConnectClient,
    effect: ConnectEffect,
) -> Result<Option<sdk::ConnectView>, sdk::SdkError> {
    match effect {
        ConnectEffect::Apply {
            session_id,
            revision,
            command,
        } => connect
            .apply_connect(session_id, revision, command)
            .await
            .map(Some),
        ConnectEffect::Cancel {
            session_id,
            revision,
        } => connect.cancel_connect(session_id, revision).await.map(Some),
        ConnectEffect::Refresh { session_id } => connect.connect_view(session_id).await,
    }
}

pub(crate) async fn run_connect_command_with_origin(
    connect: Arc<dyn sdk::ConnectClient>,
    origin: sdk::ConnectOrigin,
) -> Result<sdk::ConnectOutcome, sdk::SdkError> {
    use crossterm::event::{self, Event, KeyEventKind};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(sdk::SdkError::Init(
            "aemeath connect 需要交互终端".to_string(),
        ));
    }
    enable_raw_mode().map_err(|error| sdk::SdkError::Internal(error.to_string()))?;
    let _raw_mode = RawModeGuard;
    let view = connect.start_connect(origin).await?;
    let mut model = ConnectUiModel::new(view);
    loop {
        render(model.view(), &model.visible_input());
        if let Some(outcome) = model.view().terminal.clone() {
            return Ok(outcome);
        }
        if matches!(
            model.view().stage,
            sdk::ConnectStage::Probing | sdk::ConnectStage::Saving
        ) {
            let Some(refreshed) = execute_effect(
                connect.as_ref(),
                ConnectEffect::Refresh {
                    session_id: model.view().session_id.clone(),
                },
            )
            .await?
            else {
                return Err(sdk::SdkError::Internal("Connect 会话不存在".to_string()));
            };
            model.replace_view(refreshed);
            continue;
        }
        let Event::Key(key) =
            event::read().map_err(|error| sdk::SdkError::Internal(error.to_string()))?
        else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let Some(effect) = model.update(key) else {
            continue;
        };
        let view = execute_effect(connect.as_ref(), effect)
            .await?
            .ok_or_else(|| sdk::SdkError::Internal("Connect 会话不存在".to_string()))?;
        model.replace_view(view);
    }
}

pub(crate) async fn run_connect_command(
    connect: Arc<dyn sdk::ConnectClient>,
) -> Result<(), sdk::SdkError> {
    run_connect_command_with_origin(connect, sdk::ConnectOrigin::ExplicitCommand)
        .await
        .map(|_| ())
}

fn render(view: &sdk::ConnectView, visible_input: &str) {
    print!("\x1b[2J\x1b[H");
    for line in ConnectProjection::from_view(view).lines() {
        println!("{line}");
    }
    println!("可用操作: {:?}", view.available_actions);
    println!("Esc 取消 · Enter 提交");
    print!("> {visible_input}");
    let _ = io::stdout().flush();
}
