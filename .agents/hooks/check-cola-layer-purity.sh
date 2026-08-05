#!/bin/bash
set -euo pipefail
# guard-registry:policy.hexagonal.current-layer-matrix
# guard-registry:policy.task.target-layout

# 功能：检查未迁移 feature 的 COLA 分层，并锁定已迁移 feature 的目标目录。
# 作用：普通 feature 继续受迁移期 COLA 依赖方向约束；Runtime 使用
#       domain/application/ports/adapters/shared；Workflow 使用 domain；Storage 使用 domain/ports/adapters；
#       Project/Tools/Task 使用 domain/adapters（domain 不得依赖 adapters）；Audit 仅允许随真实 Usage 交付增量建立的 Hexagonal 层。
# 例外：RUNTIME_LAYER_MIGRATION_EXCEPTIONS 为空集合（当前无迁移期层级倒置）。
#
# 实现：perl 单进程核心（issue 1521）。原实现为 xtask 子命令 `cola-layer-purity`
#       （tools/xtask/src/cola_layer_purity.rs），Stop Hook 每次触发产生 40~80s
#       编译/运行成本；perl 为 macOS 内置，单进程全仓扫描 <1s，无编译依赖。
#       所有含 \b / (?:) / 非贪婪的正则在 perl 中语义与 Rust regex 一致。
# 历史：python heredoc 实现在部分环境 stdin 卡死导致 push 阻断（#1500），曾移植为 Rust。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi
cd "$ROOT"

AEMEATH_ROOT="$ROOT" perl - <<'PL'
use strict;
use warnings;
use JSON::PP;

# ---------- 常量矩阵（与 xtask cola_layer_purity.rs 一致） ----------
my @FEATURE_LAYERS = qw(contract gateway core business utils);
my @RUNTIME_HEX_LAYERS = qw(domain application ports adapters shared);
my @WORKFLOW_HEX_LAYERS = qw(domain);
my @PROVIDER_HEX_LAYERS = qw(domain adapters);
my @MEMORY_HEX_LAYERS = qw(domain application ports adapters);
my @PROVIDER_LEGACY_LAYERS = qw(api business contract core gateway);
my @POLICY_HEX_LAYERS = qw(domain adapters);
my @POLICY_ALLOWED_TOP_LEVEL_FILES = qw(lib.rs domain.rs adapters.rs);
my @POLICY_LEGACY_LAYERS = qw(api business contract core gateway capabilities);
my @STORAGE_HEX_LAYERS = qw(domain ports adapters);
my @STORAGE_LEGACY_LAYERS = qw(api business contract gateway memory_store task_store);
my @PROJECT_HEX_LAYERS = qw(domain adapters);
my @PROJECT_ALLOWED_TOP_LEVEL_FILES = qw(lib.rs domain.rs adapters.rs);
my @PROJECT_LEGACY_LAYERS = qw(api business contract core gateway capabilities);
my @TOOLS_HEX_LAYERS = qw(domain adapters);
my @TOOLS_ALLOWED_TOP_LEVEL_FILES = qw(lib.rs domain.rs adapters.rs);
my @TOOLS_LEGACY_LAYERS = qw(api business contract core gateway);
my @TASK_HEX_LAYERS = qw(domain adapters);
my @TASK_ALLOWED_TOP_LEVEL_FILES = qw(lib.rs domain.rs adapters.rs);
my @TASK_LEGACY_LAYERS = qw(api business contract core gateway ports capabilities);
my @AUDIT_HEX_LAYERS = qw(domain application ports adapters);
my @AUDIT_ALLOWED_TOP_LEVEL_FILES = qw(lib.rs domain.rs application.rs ports.rs adapters.rs);
my @AUDIT_LEGACY_LAYERS = qw(api business contract core gateway capabilities);
my @HOOK_HEX_LAYERS = qw(domain ports adapters);
my @HOOK_ALLOWED_TOP_LEVEL_FILES = qw(lib.rs domain.rs ports.rs adapters.rs capabilities.rs);
my @HOOK_LEGACY_LAYERS = qw(api business contract core gateway capabilities);
my @CONTEXT_HEX_LAYERS = qw(domain application ports adapters);
my @TOOL_PROFILE_PUBLIC_API = qw(baseline derive_restricted allowed_capabilities);
my $POLICY_FORBIDDEN_ADAPTER_TYPES = qr/\b(?:struct|enum)\s+(?:Deny|Approval|RequireApproval)\w*Policy\b/;

my %FORBIDDEN_LAYER_DEPS = (
  business    => [qw(core gateway contract)],
  utils       => [qw(business core gateway contract)],
  contract    => [qw(business core gateway utils)],
  gateway     => [qw(business utils)],
  domain      => [qw(application ports adapters)],
  ports       => [qw(application adapters)],
  application => [qw(adapters)],
  shared      => [qw(domain application ports adapters)],
);

my @violations;

sub contains {
  my ($item, @list) = @_;
  for my $candidate (@list) { return 1 if $candidate eq $item }
  return 0;
}

# Rust `{:?}` Debug 数组格式：["a", "b", "c"]
sub fmt_list {
  my (@list) = @_;
  return "[" . join(", ", map { "\"$_\"" } @list) . "]";
}

# ---------- 文本工具 ----------

# 与 Rust regex 一致：. 不跨行（块注释按行替换，行为与原实现相同）
sub strip_comments {
  my ($source) = @_;
  $source =~ s{/\*.*?\*/}{}g;
  $source =~ s{//.*$}{}gm;
  return $source;
}

# header 正则匹配后首个 { ... } 块体（不含外层花括号）
sub named_block {
  my ($source, $header) = @_;
  return "" unless $source =~ /$header/s;
  my $start = $+[0];
  my $opening = index($source, "{", $start);
  return "" if $opening < 0;
  my $depth = 0;
  for (my $index = $opening; $index < length($source); $index++) {
    my $ch = substr($source, $index, 1);
    $depth++ if $ch eq "{";
    $depth-- if $ch eq "}";
    return substr($source, $opening + 1, $index - $opening - 1) if $depth == 0;
  }
  return "";
}

# ---------- 违规检查函数（与 xtask 同名函数等价） ----------

# 单行依赖方向检查；返回违规消息列表
sub line_layer_violation_for {
  my ($current_layer, $line) = @_;
  my $stripped = $line;
  $stripped =~ s/^\s+|\s+$//g;
  return () if $stripped eq "" || $stripped =~ m{^//} || $stripped =~ m{^\*};
  my $forbiddens = $FORBIDDEN_LAYER_DEPS{$current_layer} || [];
  return () unless @$forbiddens;
  my @out;
  while ($line =~ /\b(?:use\s+)?crate::([A-Za-z_][A-Za-z0-9_]*)/g) {
    my $target = $1;
    if (contains($target, @$forbiddens)) {
      push @out, "feature layer $current_layer must not depend on crate::$target";
    }
  }
  return @out;
}

sub tool_profile_violations {
  my ($source) = @_;
  my $stripped = strip_comments($source);
  my @out;
  my $body = named_block($stripped, qr/\bpub\s+struct\s+ToolProfile\b/);
  if ($body ne "") {
    my @fields;
    while ($body =~ /(?:^|,)\s*(pub(?:\([^)]*\))?\s+)?allowed_capabilities\s*:/g) {
      push @fields, $&;
    }
    if (@fields != 1 || $fields[0] =~ /pub/) {
      push @out, "ToolProfile.allowed_capabilities must remain a private field";
    }
  }
  $body = named_block($stripped, qr/\bimpl\s+ToolProfile\b/);
  if ($body ne "") {
    my @public_methods = $body =~ /\bpub\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)/g;
    my @expansion = grep { !contains($_, @TOOL_PROFILE_PUBLIC_API) } @public_methods;
    if (@expansion) {
      my $names = join(", ", sort @expansion);
      push @out, "ToolProfile must not expose capability-expanding mutation API: $names";
    }
    if ($body =~ /\bfn\s+\w+\s*\([^)]*&mut\s+self/) {
      push @out, "ToolProfile must not expose in-place mutation";
    }
    if ($body =~ /self\.allowed_capabilities\s*(?:\|=|&=|\^=|=)|self\.allowed_capabilities\s*\.\s*(?:insert|extend|union)/) {
      push @out, "ToolProfile.allowed_capabilities must not be mutated";
    }
  }
  return @out;
}

sub tools_authorization_violations {
  my ($source) = @_;
  my $stripped = strip_comments($source);
  my @out;
  if ($stripped =~ /\b(?:ToolProfile|is_authorized|authoriz\w*|exclud\w*|denylist|blacklist)\b/
      && $stripped =~ /\bexcludes\b/) {
    push @out, "ToolProfile::excludes/name blacklist authorization is forbidden";
  }
  if ($stripped =~ /\b(?:exclud\w*|denylist|blacklist)\b[\s\S]{0,500}/) {
    my $name_based = $&;
    if ($name_based =~ /(?:\bmatch\s+[^{}]*?(?:\btool_?name\b|\.name\b)|\bmatches!\s*\([^,]*?(?:\bToolName\b|\btool_?name\b|\.name\b))/) {
      push @out, "authorization must not match on ToolName; use declared capabilities";
    }
  }
  return @out;
}

sub tools_boundary_violations {
  my ($rel_s, $source) = @_;
  my $stripped = strip_comments($source);
  my @out;
  if ($rel_s eq "agent/features/tools/src/lib.rs"
      && $stripped =~ /\bpub\s+(?:use\b[^;]*\b|(?:struct|enum|type)\s+)(?:RegistryScopeBuilder|RegistryScope)\b/) {
    push @out, "RegistryScopeBuilder/RegistryScope must not enter the tools crate-root facade";
  }
  if ($rel_s =~ m{^agent/features/tools/src/domain} && $stripped =~ /\bToolRegistry\b/) {
    push @out, "ToolRegistry is an adapter and must not enter tools domain";
  }
  return @out;
}

sub is_test_path {
  my ($path) = @_;
  return 1 if $path =~ /_(?:test|tests)\.rs$/;
  my $name = $path;
  $name =~ s{.*/}{};
  my $stem = $name;
  $stem =~ s/\.rs$//;
  return 1 if $stem eq "tests";
  return 1 if $path =~ m{/tests/};
  return 0;
}

# 返回 (feature, layer) 或空列表
sub feature_layer_for {
  my ($path) = @_;
  return () unless $path =~ m{^agent/features/([^/]+)/src/([^/]+)(?:/|$)};
  my ($feature, $layer) = ($1, $2);
  $layer =~ s/\.rs$//;
  my %hex_layers = (
    runtime  => \@RUNTIME_HEX_LAYERS,
    workflow => \@WORKFLOW_HEX_LAYERS,
    provider => \@PROVIDER_HEX_LAYERS,
    memory   => \@MEMORY_HEX_LAYERS,
    context  => \@CONTEXT_HEX_LAYERS,
    policy   => \@POLICY_HEX_LAYERS,
    project  => \@PROJECT_HEX_LAYERS,
    tools    => \@TOOLS_HEX_LAYERS,
    task     => \@TASK_HEX_LAYERS,
    audit    => \@AUDIT_HEX_LAYERS,
    hook     => \@HOOK_HEX_LAYERS,
  );
  if (exists $hex_layers{$feature}) {
    return ($feature, $layer) if contains($layer, @{$hex_layers{$feature}});
    return ();
  }
  return () if $feature eq "storage";
  return ($feature, $layer) if contains($layer, @FEATURE_LAYERS);
  return ();
}

# ---------- 目录分层检查（与 xtask check() 的 src_dirs 循环等价；src_dirs 本身是死变量，不移植） ----------
sub check_src_layout {
  my $features_root = "agent/features";
  return unless -d $features_root;
  opendir my $dh, $features_root or return;
  my @crate_names = sort grep { -d "$features_root/$_" } readdir $dh;
  closedir $dh;
  for my $crate_name (@crate_names) {
    my $src = "$features_root/$crate_name/src";
    next unless -d $src;
    opendir my $sdh, $src or next;
    my @children = sort grep { !/^\./ } readdir $sdh;
    closedir $sdh;
    for my $name (@children) {
      my $child = "$src/$name";
      my $rel = "agent/features/$crate_name/src/$name";
      if ($crate_name eq "runtime") {
        if (-d $child && contains($name, @FEATURE_LAYERS)) {
          push @violations, "$rel: Runtime legacy COLA directory is forbidden; use " . fmt_list(@RUNTIME_HEX_LAYERS);
          next;
        }
      } elsif ($crate_name eq "workflow") {
        if (-d $child) {
          push @violations, "$rel: Workflow source directories must be " . fmt_list(@WORKFLOW_HEX_LAYERS) unless contains($name, @WORKFLOW_HEX_LAYERS);
        } elsif ($name ne "lib.rs" && $name ne "domain.rs") {
          push @violations, "$rel: Workflow top-level source files must be lib.rs or domain.rs";
        }
        next;
      } elsif ($crate_name eq "provider") {
        if (contains($name, @PROVIDER_LEGACY_LAYERS)) {
          push @violations, "$rel: Provider legacy fixed layer is forbidden; use domain/ports/adapters";
          next;
        }
        push @violations, "$rel: Provider source directories must be " . fmt_list(@PROVIDER_HEX_LAYERS) if (-d $child && !contains($name, @PROVIDER_HEX_LAYERS));
        next;
      } elsif ($crate_name eq "policy") {
        if (contains($name, @POLICY_LEGACY_LAYERS)) {
          push @violations, "$rel: Policy legacy fixed layer is forbidden; use " . fmt_list(@POLICY_HEX_LAYERS);
        } elsif (-d $child && !contains($name, @POLICY_HEX_LAYERS)) {
          push @violations, "$rel: Policy source directories must be " . fmt_list(@POLICY_HEX_LAYERS);
        } elsif (!-d $child && !contains($name, @POLICY_ALLOWED_TOP_LEVEL_FILES)) {
          push @violations, "$rel: Policy top-level source files must be " . fmt_list(@POLICY_ALLOWED_TOP_LEVEL_FILES);
        }
        next;
      } elsif ($crate_name eq "project") {
        if (contains($name, @PROJECT_LEGACY_LAYERS)) {
          push @violations, "$rel: Project legacy fixed layer is forbidden; use " . fmt_list(@PROJECT_HEX_LAYERS);
        } elsif (-d $child && !contains($name, @PROJECT_HEX_LAYERS)) {
          push @violations, "$rel: Project source directories must be " . fmt_list(@PROJECT_HEX_LAYERS);
        } elsif (!-d $child && !contains($name, @PROJECT_ALLOWED_TOP_LEVEL_FILES)) {
          push @violations, "$rel: Project top-level source files must be " . fmt_list(@PROJECT_ALLOWED_TOP_LEVEL_FILES);
        }
        next;
      } elsif ($crate_name eq "audit") {
        if (contains($name, @AUDIT_LEGACY_LAYERS)) {
          push @violations, "$rel: Audit empty or legacy fixed layer is forbidden; use evidence-backed " . fmt_list(@AUDIT_HEX_LAYERS);
        } elsif (-d $child && !contains($name, @AUDIT_HEX_LAYERS)) {
          push @violations, "$rel: Audit source directories must be evidence-backed layers " . fmt_list(@AUDIT_HEX_LAYERS);
        } elsif (!-d $child && !contains($name, @AUDIT_ALLOWED_TOP_LEVEL_FILES)) {
          push @violations, "$rel: Audit top-level source files must be " . fmt_list(@AUDIT_ALLOWED_TOP_LEVEL_FILES);
        }
        next;
      } elsif ($crate_name eq "hook") {
        if (contains($name, @HOOK_LEGACY_LAYERS)) {
          push @violations, "$rel: Hook legacy fixed layer is forbidden; use " . fmt_list(@HOOK_HEX_LAYERS);
        } elsif (-d $child && !contains($name, @HOOK_HEX_LAYERS)) {
          push @violations, "$rel: Hook source directories must be " . fmt_list(@HOOK_HEX_LAYERS);
        } elsif (!-d $child && !contains($name, @HOOK_ALLOWED_TOP_LEVEL_FILES)) {
          push @violations, "$rel: Hook top-level source files must be " . fmt_list(@HOOK_ALLOWED_TOP_LEVEL_FILES);
        }
        next;
      } elsif ($crate_name eq "tools") {
        if (contains($name, @TOOLS_LEGACY_LAYERS)) {
          push @violations, "$rel: tools legacy fixed layer is forbidden; use " . fmt_list(@TOOLS_HEX_LAYERS);
        } elsif (-d $child && !contains($name, @TOOLS_HEX_LAYERS)) {
          push @violations, "$rel: tools source directories must be " . fmt_list(@TOOLS_HEX_LAYERS);
        } elsif (!-d $child && !contains($name, @TOOLS_ALLOWED_TOP_LEVEL_FILES)) {
          push @violations, "$rel: tools top-level source files must be " . fmt_list(@TOOLS_ALLOWED_TOP_LEVEL_FILES);
        }
        next;
      } elsif ($crate_name eq "task") {
        if (contains($name, @TASK_LEGACY_LAYERS)) {
          push @violations, "$rel: Task legacy fixed layer is forbidden; use " . fmt_list(@TASK_HEX_LAYERS);
        } elsif (-d $child && !contains($name, @TASK_HEX_LAYERS)) {
          push @violations, "$rel: Task source directories must be " . fmt_list(@TASK_HEX_LAYERS);
        } elsif (!-d $child && !contains($name, @TASK_ALLOWED_TOP_LEVEL_FILES)) {
          push @violations, "$rel: Task top-level source files must be " . fmt_list(@TASK_ALLOWED_TOP_LEVEL_FILES);
        }
        next;
      } elsif ($crate_name eq "storage") {
        if (contains($name, @STORAGE_LEGACY_LAYERS)) {
          push @violations, "$rel: Storage legacy fixed layer is forbidden; use " . fmt_list(@STORAGE_HEX_LAYERS);
        } elsif (-d $child && !contains($name, @STORAGE_HEX_LAYERS)) {
          push @violations, "$rel: Storage directory must be a hexagonal layer " . fmt_list(@STORAGE_HEX_LAYERS) . " or registered transitional module";
        }
        next;
      } elsif ($crate_name eq "memory") {
        next if (-d $child && contains($name, @MEMORY_HEX_LAYERS));
      }
      if (-d $child && !contains($name, @FEATURE_LAYERS)) {
        next if ($crate_name eq "runtime" && contains($name, @RUNTIME_HEX_LAYERS));
        next if ($crate_name eq "context" && contains($name, @CONTEXT_HEX_LAYERS));
        push @violations, "$rel: feature src directories must be COLA layers " . fmt_list(@FEATURE_LAYERS);
      }
    }
  }
}

# ---------- 文件级检查 ----------
sub collect_rs_files {
  my ($dir, $out) = @_;
  opendir my $dh, $dir or return;
  my @entries = readdir $dh;
  closedir $dh;
  for my $entry (sort @entries) {
    next if $entry eq "." || $entry eq "..";
    my $path = "$dir/$entry";
    if (-d $path) {
      collect_rs_files($path, $out);
    } elsif ($path =~ /\.rs$/) {
      push @$out, $path;
    }
  }
}

sub check_files {
  my $features_root = "agent/features";
  return unless -d $features_root;
  my @rs_files;
  collect_rs_files($features_root, \@rs_files);
  for my $rel_s (@rs_files) {
    next if is_test_path($rel_s);
    my $source = do {
      open my $fh, "<", $rel_s or next;
      local $/;
      my $content = <$fh>;
      close $fh;
      $content;
    };
    if ($rel_s =~ m{^agent/features/tools/src/}) {
      for my $violation (tools_authorization_violations($source)) {
        push @violations, "$rel_s: $violation";
      }
      for my $violation (tools_boundary_violations($rel_s, $source)) {
        push @violations, "$rel_s: $violation";
      }
      if (strip_comments($source) =~ /\bpub\s+struct\s+ToolProfile\b/) {
        for my $violation (tool_profile_violations($source)) {
          push @violations, "$rel_s: $violation";
        }
      }
    }
    if (($rel_s =~ m{^agent/features/storage/src/domain/} || $rel_s eq "agent/features/storage/src/domain.rs")
        && $source =~ /\b(?:std|tokio)::fs::|\bPathBuf\b|\bcrate::adapters\b/) {
      push @violations, "$rel_s: Storage domain must not perform physical I/O, own PathBuf, or depend on adapters";
    }
    my @feature_layer = feature_layer_for($rel_s);
    next unless @feature_layer;
    my ($feature, $layer) = @feature_layer;
    if ($layer eq "domain"
        && $rel_s =~ m{^agent/features/project/src/}
        && $source =~ /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/) {
      push @violations, "$rel_s: Project domain must not depend on crate::adapters";
    }
    my $lineno = 0;
    for my $line (split /\n/, $source) {
      $lineno++;
      my $trimmed = $line;
      $trimmed =~ s/^\s+|\s+$//g;
      for my $violation (line_layer_violation_for($layer, $line)) {
        push @violations, "$rel_s:$lineno: $violation: $trimmed";
      }
    }
  }
}

# ---------- 自检（与 xtask run_sanity() 的 23 项断言等价） ----------
sub run_sanity {
  my $fail = 0;
  my $check = sub {
    my ($cond, $message) = @_;
    unless ($cond) {
      print STDERR "sanity: $message\n";
      $fail = 1;
    }
  };
  # line_layer_violations 断言
  $check->(@{ [line_layer_violation_for("business", "use crate::core::port::ToolPort;")] } != 0, "business->core 应被阻断");
  $check->(@{ [line_layer_violation_for("utils", "let _ = crate::business::Policy::default();")] } != 0, "utils->business 应被阻断");
  $check->(@{ [line_layer_violation_for("core", "use crate::business::TaskState;")] } == 0, "core->business 应放行");
  $check->(@{ [line_layer_violation_for("domain", "use crate::application::Agent;")] } != 0, "domain->application 应被阻断");
  $check->(@{ [line_layer_violation_for("application", "use crate::adapters::SdkProjection;")] } != 0, "application->adapters 应被阻断");
  $check->(@{ [line_layer_violation_for("application", "use crate::domain::Run;")] } == 0, "application->domain 应放行");
  $check->(@{ [line_layer_violation_for("adapters", "use crate::ports::EventSink;")] } == 0, "adapters->ports 应放行");
  $check->(@{ [line_layer_violation_for("business", "use crate::utils::normalize_path;")] } == 0, "business->utils 应放行");
  $check->(@{ [line_layer_violation_for("domain", "use crate::adapters::git::GitCli;")] } != 0, "domain->adapters 应被阻断");
  $check->(@{ [line_layer_violation_for("adapters", "use crate::domain::git::GitWorktreeOps;")] } == 0, "adapters->domain 应放行");
  # project_domain_adapter_pattern 断言（多行）
  $check->("use crate::{\n adapters::git::GitCli,\n};" =~ /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/, "多行 braced Project domain 依赖应被阻断");
  $check->("use crate::{\n domain::types::WorkspaceRead,\n adapters::git::GitCli,\n};" =~ /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/, "非首个 braced Project domain 依赖应被阻断");
  $check->("use crate::\n adapters::git::GitCli;" =~ /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/, "多行 Project domain 依赖应被阻断");
  # tool_profile_violations 断言
  my $safe_profile = 'pub struct ToolProfile { allowed_capabilities: ToolCapabilities }
impl ToolProfile {
    pub fn baseline(value: ToolCapabilities) -> Self { Self { allowed_capabilities: value } }
    pub fn derive_restricted(parent: &Self, requested: ToolCapabilities) -> Self {
        Self::baseline(requested & parent.allowed_capabilities)
    }
    pub fn allowed_capabilities(&self) -> ToolCapabilities { self.allowed_capabilities }
}';
  $check->(@{ [tool_profile_violations($safe_profile)] } == 0, "私有 shrink-only ToolProfile 应放行");
  $check->(@{ [tool_profile_violations("pub struct ToolProfile { pub allowed_capabilities: ToolCapabilities }")] } != 0, "公开 ToolProfile 字段应被阻断");
  my $expanding_profile = $safe_profile;
  $expanding_profile =~ s/pub fn allowed_capabilities/pub fn insert(\&mut self, value: ToolCapabilities) { self.allowed_capabilities |= value; } pub fn allowed_capabilities/;
  $check->(@{ [tool_profile_violations($expanding_profile)] } != 0, "capability-expanding ToolProfile API 应被阻断");
  # tools_authorization_violations 断言
  $check->(@{ [tools_authorization_violations("impl ToolProfile { fn excludes(&self, name: &ToolName) -> bool { matches!(name, ToolName::Bash) } }")] } != 0, "ToolProfile name blacklist 应被阻断");
  $check->(@{ [tools_authorization_violations("fn is_authorized(required: Caps, profile: ToolProfile) -> bool { required.is_subset_of(profile.allowed_capabilities()) }")] } == 0, "capability authorization 应放行");
  # tools_boundary_violations 断言
  $check->(@{ [tools_boundary_violations("agent/features/tools/src/lib.rs", "pub use domain::RegistryScopeBuilder;")] } != 0, "RegistryScopeBuilder 进 crate-root facade 应被阻断");
  $check->(@{ [tools_boundary_violations("agent/features/tools/src/domain/catalog.rs", "use crate::adapters::ToolRegistry;")] } != 0, "ToolRegistry 进 domain 应被阻断");
  $check->(@{ [tools_boundary_violations("agent/features/tools/src/adapters/catalog.rs", "use super::ToolRegistry;")] } == 0, "ToolRegistry 在 adapters 应放行");
  if ($fail) {
    print STDERR "check-cola-layer-purity: sanity 自检失败\n";
    exit 1;
  }
}

# ---------- 主流程（与 xtask check() 等价） ----------
run_sanity;

# policy/src/adapters.rs 生产 adapter 必须 AllowAll-only
if (-f "agent/features/policy/src/adapters.rs") {
  my $policy_adapter = do {
    open my $fh, "<", "agent/features/policy/src/adapters.rs" or die "open policy adapters: $!";
    local $/;
    my $content = <$fh>;
    close $fh;
    $content;
  };
  if (strip_comments($policy_adapter) =~ $POLICY_FORBIDDEN_ADAPTER_TYPES) {
    push @violations, "agent/features/policy/src/adapters.rs: v0.1.0 production adapter must be AllowAll-only";
  }
}

# 旧目录必须不存在
for my $old_path (qw(agent/runtime agent/provider agent/tools)) {
  if (-e $old_path) {
    push @violations, "$old_path: runtime/provider/tools must live under agent/features/*";
  }
}

check_src_layout();
check_files();

if (@violations) {
  my $reason = "COLA layer purity guard FAILED:\n" . join("\n", @violations);
  print JSON::PP->new->canonical->encode({ decision => "block", reason => $reason }), "\n";
  STDOUT->flush;
  print STDERR "$reason\n";
  exit 1;
}
PL
