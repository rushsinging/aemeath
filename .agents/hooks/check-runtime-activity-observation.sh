#!/bin/bash
# guard-registry:policy.runtime.activity-observation
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

perl - "$ROOT" <<'PERL'
use strict;
use warnings;
use File::Find;
use File::Spec;

my $root = shift @ARGV;
my $runtime = "$root/agent/features/runtime/src";
my $tui = "$root/apps/cli/src/tui";
my $coordinator = "$runtime/application/activity/coordinator.rs";
my $model = "$runtime/application/activity/model.rs";
my $root_reducer = "$tui/update/root_reducer.rs";
my $runtime_activity_events = "$runtime/application/loop_engine/chat/events.rs";
my $sdk_event_sink = "$runtime/adapters/sdk_event_sink.rs";
my @violations;

sub rust_files {
  my (@roots) = @_;
  my @paths;
  for my $search_root (@roots) {
    find(
      {
        no_chdir => 1,
        wanted => sub {
          push @paths, $File::Find::name if -f $_ && $_ =~ /\.rs\z/;
        },
      },
      $search_root,
    );
  }
  return sort @paths;
}

sub production_text {
  my ($path) = @_;
  open my $source_file, '<', $path or die "cannot read $path: $!";
  local $/;
  my $source = <$source_file>;
  close $source_file;
  $source =~ s/#\[cfg\(test\)\].*?\n}\n//sg;
  return join "\n", grep { $_ !~ /^\s*\/\// } split /\n/, $source;
}

sub relative_path {
  my ($path) = @_;
  return File::Spec->abs2rel($path, $root);
}

sub is_test {
  my ($path) = @_;
  my @parts = File::Spec->splitdir($path);
  return 1 if grep { $_ eq 'tests' } @parts;
  my ($volume, $directories, $file) = File::Spec->splitpath($path);
  $file =~ s/\.rs\z//;
  return $file =~ /test/;
}

for my $path (rust_files($runtime)) {
  next if is_test($path) || $path eq $coordinator || $path eq $model;
  if (index(production_text($path), 'ActivityObservation {') >= 0) {
    push @violations,
      'Runtime ActivityObservation construction must stay in ActivityCoordinator: '
      . relative_path($path);
  }
}

for my $path (rust_files($tui)) {
  next if is_test($path) || $path =~ m{/scenario_tests/} || $path eq $root_reducer;
  if (index(production_text($path), 'activity_observations_mut(') >= 0) {
    push @violations,
      'TUI ActivityObservation mutation must stay behind root reducer: '
      . relative_path($path);
  }
}

my $live_status = production_text("$tui/view_assembler/live_status.rs");
for my $symbol ('RunStatusView', 'TuiRunStatus', 'RunTransitioned') {
  push @violations, "LiveStatus must not depend on legacy Run status: $symbol"
    if index($live_status, $symbol) >= 0;
}

my @legacy_symbols = (
  'RunStatusObserved',
  'RunStateSnapshot',
  'run_state_snapshots',
  'active_main_run_snapshot',
  'TuiRunTiming',
  'SpinnerPhase',
  'chat_active',
  'running_tool_count',
);
for my $path (rust_files($tui)) {
  next if is_test($path) || $path =~ m{/architecture_tests\.rs\z};
  my $source = production_text($path);
  for my $symbol (@legacy_symbols) {
    push @violations,
      "legacy Activity display symbol $symbol: " . relative_path($path)
      if index($source, $symbol) >= 0;
  }
}

my $runtime_activity_source = production_text($runtime_activity_events) . production_text($sdk_event_sink);
for my $symbol ('RuntimeActivityEvent::Changed', 'publish_change(') {
  push @violations, "Runtime production Activity must publish logical-commit Snapshot only: $symbol"
    if index($runtime_activity_source, $symbol) >= 0;
}

my @hook_parallel_symbols = (
  'RuntimeStreamEvent::HookEvent',
  'RuntimeStreamEvent::HookMessage',
  'RuntimeStreamEvent::StopHookBlocked',
  'ChatEvent::HookEvent',
  'ChatEvent::HookMessage',
  'ChatEvent::StopHookBlocked',
  'TuiRuntimeEvent::HookEvent',
  'TuiRuntimeEvent::HookMessage',
  'TuiRuntimeEvent::StopHookBlocked',
  'UiEvent::HookEvent',
  'UiEvent::HookMessage',
  'UiEvent::StopHookBlocked',
);
for my $path (rust_files($runtime, "$root/packages/sdk/src", $tui)) {
  next if is_test($path) || $path =~ m{/scenario_tests/};
  my $source = production_text($path);
  for my $symbol (@hook_parallel_symbols) {
    push @violations,
      "Hook display must use the unique Activity observation path, found $symbol: "
      . relative_path($path)
      if index($source, $symbol) >= 0;
  }
}

my %allowed_direct_hook_dispatches = map { $_ => 1 } (
  "$runtime/application/loop_engine/chat/hook_ui.rs",
  "$runtime/application/hook/stop_coordination.rs",
  "$runtime/application/hook/empty.rs",
  "$runtime/application/prompt/build/prompt_build.rs",
  "$runtime/application/prompt/instructions_hook.rs",
);
for my $path (rust_files($runtime)) {
  next if is_test($path) || $allowed_direct_hook_dispatches{$path};
  if (index(production_text($path), '.dispatch_at(') >= 0) {
    push @violations,
      'Run Hook dispatch must publish an Activity lifecycle through the designated boundary: '
      . relative_path($path);
  }
}

for my $path (
  $coordinator,
  "$tui/effect/session/processing/logging.rs",
) {
  my $source = production_text($path);
  for my $field (
    'run_id={}',
    'revision={}',
    'total_elapsed_ms={}',
    'active_elapsed_ms={}',
    'state_elapsed_ms={}',
  ) {
    push @violations,
      "Activity diagnostic missing $field: " . relative_path($path)
      if index($source, $field) < 0;
  }
  for my $sensitive ('raw_args', 'stdout', 'response={}') {
    push @violations,
      "Activity diagnostic exposes $sensitive: " . relative_path($path)
      if index($source, $sensitive) >= 0;
  }
}

if (@violations) {
  print STDERR "[architecture] Runtime Activity observation guard failed:\n";
  print STDERR "  - $_\n" for @violations;
  exit 2;
}

print "Runtime Activity observation guard passed.\n";
PERL
