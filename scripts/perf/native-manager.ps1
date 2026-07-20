param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ColdRunCount = 5
$ColdExitAfterMs = 3000
$IdleSampleSeconds = 30
$IdleExitAfterMs = 58000
$FirstFrameLimitMs = 1500.0
$CpuP95LimitMs = 16.7
$MaximumStallLimitMs = 50.0
$PrivateMemoryLimitBytes = 157286400L
$ExpectedScriptActions = @(
    'navigate_providers',
    'select_next_provider',
    'edit_provider_name',
    'discard_provider',
    'refresh_live',
    'open_live_tab',
    'request_clear_live',
    'cancel_live_confirmation',
    'request_clear_live',
    'confirm_live_mutation',
    'toggle_provider_list',
    'navigate_environment',
    'refresh_environment',
    'select_environment_conflict',
    'request_environment_cleanup',
    'cancel_environment_cleanup',
    'navigate_providers',
    'open_ccs_import',
    'close_ccs_import',
    'navigate_overview',
    'navigate_context',
    'refresh_context',
    'select_next_context_kind',
    'create_context_entry',
    'cancel_context_editor',
    'open_first_context_entry',
    'cancel_context_editor',
    'toggle_first_context_entry',
    'request_delete_first_context_entry',
    'cancel_context_delete',
    'preview_context_sync',
    'cancel_context_sync_preview',
    'preview_context_sync',
    'confirm_context_sync',
    'refresh_marketplace',
    'request_local_marketplace_repair',
    'confirm_local_marketplace_repair',
    'request_remote_marketplace_repair',
    'confirm_remote_marketplace_repair',
    'navigate_sessions',
    'refresh_sessions',
    'set_session_query',
    'select_all_filtered_sessions',
    'open_delete_confirmation',
    'cancel_delete_confirmation',
    'run_provider_repair',
    'cancel_provider_repair',
    'navigate_scripts',
    'refresh_local_scripts',
    'refresh_script_market',
    'open_local_scripts',
    'open_script_market',
    'request_verified_script_install',
    'cancel_script_install',
    'request_verified_script_install',
    'confirm_verified_script_install',
    'open_local_scripts',
    'disable_all_scripts',
    'toggle_first_user_script',
    'request_script_conflict',
    'retry_script_conflict',
    'request_delete_first_user_script',
    'cancel_user_script_delete',
    'request_delete_first_user_script',
    'confirm_user_script_delete',
    'navigate_zed_remote',
    'refresh_zed_remote',
    'edit_zed_preferences',
    'save_zed_preferences',
    'request_zed_open',
    'cancel_zed_open',
    'request_zed_open',
    'confirm_zed_open',
    'request_zed_forget',
    'cancel_zed_forget',
    'request_zed_forget',
    'confirm_zed_forget',
    'request_zed_conflict_refresh',
    'confirm_zed_conflict_refresh',
    'navigate_maintenance',
    'refresh_maintenance',
    'request_desktop_repair',
    'cancel_desktop_repair',
    'request_desktop_repair',
    'confirm_desktop_repair',
    'migrate_start_at_sign_in',
    'disable_start_at_sign_in',
    'enable_start_at_sign_in',
    'set_maintenance_log_limit',
    'open_maintenance_report',
    'edit_maintenance_path',
    'save_maintenance_path',
    'pick_maintenance_executable',
    'launch_maintenance',
    'navigate_settings',
    'edit_stepwise_settings',
    'test_stepwise_settings',
    'save_stepwise_settings',
    'open_image_overlay_settings',
    'pick_overlay_image',
    'save_image_overlay_settings',
    'request_image_overlay_reset',
    'cancel_image_overlay_reset',
    'open_extra_args_settings',
    'edit_extra_args_settings',
    'save_extra_args_settings',
    'navigate_enhancements',
    'edit_enhancements',
    'save_enhancements'
)

$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$BinaryPath = Join-Path $RepositoryRoot 'target\release\codex-plus-plus-manager-native.exe'
$PerfRoot = Join-Path $RepositoryRoot 'target\perf\native-manager'
$RunDirectory = Join-Path $PerfRoot ((Get-Date -Format 'yyyyMMdd-HHmmss-fff') + "-$PID")

function Invoke-CargoBuild {
    Push-Location $RepositoryRoot
    try {
        & cargo build -p codex-plus-manager-native --release --jobs 1
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

function Wait-ForReport {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    $Deadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path -LiteralPath $Path)) {
        if ([DateTime]::UtcNow -ge $Deadline) {
            throw "performance report was not written: $Path"
        }
        Start-Sleep -Milliseconds 50
    }
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Stop-OwnedProcess {
    param(
        [Parameter(Mandatory)]
        [System.Diagnostics.Process] $Process
    )

    if ($Process.HasExited) {
        return
    }
    $null = $Process.CloseMainWindow()
    if (-not $Process.WaitForExit(3000)) {
        $Process.Kill()
        $Process.WaitForExit()
    }
}

function New-MaintenanceSettingsFixture {
    param(
        [Parameter(Mandatory)]
        [string] $SampleDirectory
    )

    $FixtureRoot = Join-Path $SampleDirectory 'maintenance-settings'
    $AppPath = Join-Path $FixtureRoot 'private-path-sentinel-codex.exe'
    $InitialImagePath = Join-Path $FixtureRoot 'initial-private-path-sentinel.png'
    $SelectedImagePath = Join-Path $FixtureRoot 'selected-private-path-sentinel.png'
    $DiagnosticLogPath = Join-Path $FixtureRoot 'diagnostic.jsonl'
    $LatestStatusPath = Join-Path $FixtureRoot 'latest-status.json'
    $LegacyWatcherSentinelPath = Join-Path $FixtureRoot 'watcher.disabled'
    $DesktopIntegrationRecordPath = Join-Path $FixtureRoot 'desktop-integration.record'
    $PathPickerResponsesPath = Join-Path $FixtureRoot 'path-picker-responses.json'
    $PathPickerRecordPath = Join-Path $FixtureRoot 'path-picker-record.json'
    $StepwiseRecordPath = Join-Path $FixtureRoot 'stepwise-test-record.json'
    $CodexLaunchRecordPath = Join-Path $FixtureRoot 'codex-launch-record.json'
    $EntrypointMutationPath = Join-Path $FixtureRoot 'entrypoint-mutation.json'
    $WatcherMutationPath = Join-Path $FixtureRoot 'watcher-mutation.json'
    $RealLaunchArtifactPath = Join-Path $FixtureRoot 'real-launch-artifact.json'
    $UpdateMetadataPath = Join-Path $FixtureRoot 'update-metadata.json'
    $UpdateAssetPath = Join-Path $FixtureRoot 'update-asset.bin'
    $UpdateLaunchRecordPath = Join-Path $FixtureRoot 'update-launch.record'
    $UpdateCheckRecordPath = Join-Path $FixtureRoot 'update-check.record'
    $SecretSentinel = 'private-stepwise-key-sentinel-7d44'
    $InitialStepwiseUrl = 'https://private-stepwise.example.test/body-sentinel-initial'
    $SavedStepwiseUrl = 'https://perf-stepwise.example.test/v2'
    $InitialStepwiseModel = 'private-body-sentinel-model'
    $SavedStepwiseModel = 'perf-stepwise-model-edited'
    $SavedStepwiseEnvironment = 'OPENAI_CODEX_PLUS_PERF_SENTINEL'
    $SavedExtraArgs = @('--perf-mode', '--safe-value=fixture')

    New-Item -ItemType Directory -Path $FixtureRoot | Out-Null
    [IO.File]::WriteAllText($AppPath, 'isolated-codex-fixture', [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText(
        $InitialImagePath,
        'isolated-initial-image-fixture',
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        $SelectedImagePath,
        'isolated-selected-image-fixture',
        [Text.UTF8Encoding]::new($false)
    )
    $DiagnosticRecord = [ordered]@{
        timestamp_ms = 123
        pid = 0
        event = 'native_manager.perf_fixture'
        detail = [ordered]@{ status = 'ready' }
    } | ConvertTo-Json -Depth 4 -Compress
    [IO.File]::WriteAllText(
        $DiagnosticLogPath,
        $DiagnosticRecord + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
    $LatestStatus = [ordered]@{
        status = 'running'
        message = 'fixture-ready'
        started_at_ms = 123
        debug_port = 9229
        helper_port = 57321
        codex_app = $null
    } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText(
        $LatestStatusPath,
        $LatestStatus,
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        $LegacyWatcherSentinelPath,
        'disabled',
        [Text.UTF8Encoding]::new($false)
    )
    $PickerResponses = [ordered]@{
        maintenance_executable = $AppPath
        settings_overlay_image = $SelectedImagePath
    } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText(
        $PathPickerResponsesPath,
        $PickerResponses,
        [Text.UTF8Encoding]::new($false)
    )
    $UpdateMetadata = [ordered]@{
        version = '0.0.0'
        body = 'isolated current release fixture'
    } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText(
        $UpdateMetadataPath,
        $UpdateMetadata,
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        $UpdateAssetPath,
        'isolated-update-asset',
        [Text.UTF8Encoding]::new($false)
    )

    [pscustomobject]@{
        FixtureRoot = $FixtureRoot
        AppPath = $AppPath
        InitialImagePath = $InitialImagePath
        SelectedImagePath = $SelectedImagePath
        DiagnosticLogPath = $DiagnosticLogPath
        LatestStatusPath = $LatestStatusPath
        LegacyWatcherSentinelPath = $LegacyWatcherSentinelPath
        DesktopIntegrationRecordPath = $DesktopIntegrationRecordPath
        PathPickerResponsesPath = $PathPickerResponsesPath
        PathPickerRecordPath = $PathPickerRecordPath
        StepwiseRecordPath = $StepwiseRecordPath
        StepwiseResult = 'ok:4'
        CodexLaunchRecordPath = $CodexLaunchRecordPath
        EntrypointMutationPath = $EntrypointMutationPath
        WatcherMutationPath = $WatcherMutationPath
        RealLaunchArtifactPath = $RealLaunchArtifactPath
        UpdateMetadataPath = $UpdateMetadataPath
        UpdateAssetPath = $UpdateAssetPath
        UpdateLaunchRecordPath = $UpdateLaunchRecordPath
        UpdateCheckRecordPath = $UpdateCheckRecordPath
        SecretSentinel = $SecretSentinel
        InitialStepwiseUrl = $InitialStepwiseUrl
        SavedStepwiseUrl = $SavedStepwiseUrl
        InitialStepwiseModel = $InitialStepwiseModel
        SavedStepwiseModel = $SavedStepwiseModel
        SavedStepwiseEnvironment = $SavedStepwiseEnvironment
        SavedExtraArgs = $SavedExtraArgs
    }
}

function New-ProviderSettingsFixture {
    param(
        [Parameter(Mandatory)]
        [string] $Path,

        [Parameter(Mandatory)]
        [pscustomobject] $MaintenanceSettingsFixture
    )

    $ProfileDefaults = [ordered]@{
        protocol = 'responses'
        relayMode = 'pureApi'
        testModel = 'perf-model'
        configContents = ''
        authContents = ''
        useCommonConfig = $true
        contextSelection = [ordered]@{ mcpServers = @(); skills = @(); plugins = @() }
        contextSelectionInitialized = $false
        contextWindow = '200000'
        autoCompactLimit = '160000'
        modelInsertMode = 'patch'
        modelList = "perf-model`nperf-model-fast"
        modelWindows = '{"perf-model":"200K"}'
        userAgent = ''
    }
    $First = [ordered]@{
        id = 'perf-provider-a'
        name = 'Performance provider A'
        model = 'perf-model'
        upstreamBaseUrl = 'https://api.example.test/v1'
    }
    $Second = [ordered]@{
        id = 'perf-provider-b'
        name = 'Performance provider B'
        model = 'perf-model-fast'
        upstreamBaseUrl = 'https://backup.example.test/v1'
    }
    foreach ($Key in $ProfileDefaults.Keys) {
        $First[$Key] = $ProfileDefaults[$Key]
        $Second[$Key] = $ProfileDefaults[$Key]
    }
    $ContextConfig = @'
[mcp_servers.alpha]
command = "context-fixture"
enabled = true

[skills.review]
path = "C:/fixture/review"
enabled = true

[plugins.lint]
enabled = true
'@
    $Settings = [ordered]@{
        codexAppPath = $MaintenanceSettingsFixture.AppPath
        codexExtraArgs = @('--private-body-sentinel-initial')
        codexAppStepwiseEnabled = $false
        codexAppStepwiseDirectSend = $false
        codexAppStepwiseBaseUrl = $MaintenanceSettingsFixture.InitialStepwiseUrl
        codexAppStepwiseApiKey = $MaintenanceSettingsFixture.SecretSentinel
        codexAppStepwiseApiKeyEnv = 'CODEX_STEPWISE_API_KEY'
        codexAppStepwiseModel = $MaintenanceSettingsFixture.InitialStepwiseModel
        codexAppStepwiseMaxItems = 6
        codexAppStepwiseMaxInputChars = 6000
        codexAppStepwiseMaxOutputTokens = 500
        codexAppStepwiseTimeoutMs = 8000
        codexAppImageOverlayEnabled = $true
        codexAppImageOverlayPath = $MaintenanceSettingsFixture.InitialImagePath
        codexAppImageOverlayOpacity = 41
        codexAppImageOverlayFitMode = 'fill'
        relayProfilesEnabled = $true
        activeRelayId = 'perf-provider-a'
        relayProfiles = @($First, $Second)
        aggregateRelayProfiles = @()
        relayCommonConfigContents = ''
        relayContextConfigContents = $ContextConfig
        relayTestModel = 'perf-model'
        zedRemoteOpenStrategy = 'reuseWindow'
        zedRemoteProjectRegistryEnabled = $false
        futureZedRoot = [ordered]@{
            keep = $true
            label = 'preserved'
        }
        futureSettingsRoot = [ordered]@{
            keep = $true
            label = 'maintenance-settings-preserved'
        }
    }
    $Json = $Settings | ConvertTo-Json -Depth 8
    [IO.File]::WriteAllText($Path, $Json, [Text.UTF8Encoding]::new($false))
    return [pscustomobject]@{
        RelayProfiles = @(
            [pscustomobject]$First
            [pscustomobject]$Second
        )
    }
}

function Assert-MaintenanceSettingsFixtureSetup {
    param(
        [Parameter(Mandatory)]
        [pscustomobject] $Fixture,

        [Parameter(Mandatory)]
        [string] $SampleDirectory,

        [Parameter(Mandatory)]
        [string] $SettingsPath
    )

    $SampleRoot = [IO.Path]::GetFullPath($SampleDirectory).TrimEnd('\') + '\'
    foreach ($FixturePath in @(
        $SettingsPath,
        $Fixture.FixtureRoot,
        $Fixture.AppPath,
        $Fixture.InitialImagePath,
        $Fixture.SelectedImagePath,
        $Fixture.DiagnosticLogPath,
        $Fixture.LatestStatusPath,
        $Fixture.LegacyWatcherSentinelPath,
        $Fixture.DesktopIntegrationRecordPath,
        $Fixture.PathPickerResponsesPath,
        $Fixture.PathPickerRecordPath,
        $Fixture.StepwiseRecordPath,
        $Fixture.CodexLaunchRecordPath,
        $Fixture.EntrypointMutationPath,
        $Fixture.WatcherMutationPath,
        $Fixture.RealLaunchArtifactPath
        $Fixture.UpdateMetadataPath
        $Fixture.UpdateAssetPath
        $Fixture.UpdateLaunchRecordPath
        $Fixture.UpdateCheckRecordPath
    )) {
        $Resolved = [IO.Path]::GetFullPath([string]$FixturePath)
        if (-not $Resolved.StartsWith($SampleRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "maintenance/settings fixture escaped the isolated sample root: $Resolved"
        }
    }

    foreach ($RequiredFile in @(
        $SettingsPath,
        $Fixture.AppPath,
        $Fixture.InitialImagePath,
        $Fixture.SelectedImagePath,
        $Fixture.DiagnosticLogPath,
        $Fixture.LatestStatusPath,
        $Fixture.LegacyWatcherSentinelPath,
        $Fixture.PathPickerResponsesPath
        $Fixture.UpdateMetadataPath
        $Fixture.UpdateAssetPath
    )) {
        if (-not (Test-Path -LiteralPath $RequiredFile -PathType Leaf)) {
            throw "maintenance/settings fixture file is missing: $RequiredFile"
        }
    }

    foreach ($UnexpectedRecord in @(
        $Fixture.PathPickerRecordPath,
        $Fixture.StepwiseRecordPath,
        $Fixture.CodexLaunchRecordPath,
        $Fixture.DesktopIntegrationRecordPath,
        $Fixture.EntrypointMutationPath,
        $Fixture.WatcherMutationPath,
        $Fixture.RealLaunchArtifactPath
        $Fixture.UpdateLaunchRecordPath
        $Fixture.UpdateCheckRecordPath
    )) {
        if (Test-Path -LiteralPath $UnexpectedRecord) {
            throw "maintenance/settings fixture record was not clean: $UnexpectedRecord"
        }
    }

    $ExpectedEnvironment = [ordered]@{
        CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH = $Fixture.DiagnosticLogPath
        CODEX_PLUS_NATIVE_LATEST_STATUS_PATH = $Fixture.LatestStatusPath
        CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_FIXTURE_STATE = 'windows_needs_repair_legacy'
        CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_RECORD_PATH = $Fixture.DesktopIntegrationRecordPath
        CODEX_PLUS_NATIVE_ENTRYPOINT_SILENT_INSTALLED = '1'
        CODEX_PLUS_NATIVE_ENTRYPOINT_MANAGEMENT_INSTALLED = '0'
        CODEX_PLUS_NATIVE_CODEX_LAUNCH_RECORD_PATH = $Fixture.CodexLaunchRecordPath
        CODEX_PLUS_NATIVE_PATH_PICKER_RESPONSES_PATH = $Fixture.PathPickerResponsesPath
        CODEX_PLUS_NATIVE_PATH_PICKER_RECORD_PATH = $Fixture.PathPickerRecordPath
        CODEX_PLUS_NATIVE_STEPWISE_TEST_RECORD_PATH = $Fixture.StepwiseRecordPath
        CODEX_PLUS_NATIVE_STEPWISE_TEST_RESULT = $Fixture.StepwiseResult
        CODEX_PLUS_NATIVE_UPDATE_METADATA_PATH = $Fixture.UpdateMetadataPath
        CODEX_PLUS_NATIVE_UPDATE_ASSET_PATH = $Fixture.UpdateAssetPath
        CODEX_PLUS_NATIVE_UPDATE_LAUNCH_RECORD_PATH = $Fixture.UpdateLaunchRecordPath
        CODEX_PLUS_NATIVE_UPDATE_CHECK_RECORD_PATH = $Fixture.UpdateCheckRecordPath
    }
    foreach ($Name in $ExpectedEnvironment.Keys) {
        $Actual = [Environment]::GetEnvironmentVariable($Name, 'Process')
        if ([string]::IsNullOrWhiteSpace($Actual) -or $Actual -ne $ExpectedEnvironment[$Name]) {
            throw "maintenance/settings isolation variable is missing or partial: $Name"
        }
    }

    $PickerResponses = Get-Content -LiteralPath $Fixture.PathPickerResponsesPath -Raw |
        ConvertFrom-Json
    if (
        $PickerResponses.maintenance_executable -ne $Fixture.AppPath -or
        $PickerResponses.settings_overlay_image -ne $Fixture.SelectedImagePath
    ) {
        throw 'path-picker responses do not match the isolated fixture files'
    }
}

function New-CodexHomeFixture {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    New-Item -ItemType Directory -Path $Path | Out-Null
    $Config = @'
model = "perf-model"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://api.example.test/v1"
'@
    [IO.File]::WriteAllText(
        (Join-Path $Path 'config.toml'),
        $Config,
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        (Join-Path $Path 'auth.json'),
        '{"OPENAI_API_KEY":"perf-placeholder"}',
        [Text.UTF8Encoding]::new($false)
    )

    $MarketplaceRoot = Join-Path $Path '.tmp\plugins'
    $MarketplaceMetadata = Join-Path $MarketplaceRoot '.agents\plugins'
    $MarketplacePlugin = Join-Path $MarketplaceRoot 'plugins\gmail'
    New-Item -ItemType Directory -Path $MarketplaceMetadata -Force | Out-Null
    New-Item -ItemType Directory -Path $MarketplacePlugin -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $MarketplaceMetadata 'marketplace.json'),
        '{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}',
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        (Join-Path $MarketplacePlugin 'SKILL.md'),
        '# Performance marketplace fixture',
        [Text.UTF8Encoding]::new($false)
    )

    New-SessionDatabaseFixture -CodexHome $Path
}

function New-SessionDatabaseFixture {
    param(
        [Parameter(Mandatory)]
        [string] $CodexHome
    )

    $SqliteDirectory = Join-Path $CodexHome 'sqlite'
    $DatabasePath = Join-Path $SqliteDirectory 'codex-dev.db'
    New-Item -ItemType Directory -Path $SqliteDirectory -Force | Out-Null

    $Python = Get-Command python -ErrorAction SilentlyContinue
    if ($null -eq $Python) {
        throw 'python is required to create the isolated SQLite performance fixture'
    }
    $CreateFixture = @'
import sqlite3
import sys

path = sys.argv[1]
connection = sqlite3.connect(path)
connection.execute(
    """CREATE TABLE threads (
        id TEXT PRIMARY KEY,
        rollout_path TEXT,
        title TEXT,
        cwd TEXT,
        model_provider TEXT,
        archived INTEGER,
        updated_at_ms INTEGER
    )"""
)
connection.executemany(
    "INSERT INTO threads VALUES (?, '', ?, ?, ?, ?, ?)",
    [
        ('perf-session-a', 'Performance session A', 'C:/perf/workspace-a', 'custom', 0, 2000),
        ('perf-session-b', 'Performance session B', 'C:/perf/workspace-b', 'custom', 1, 1000),
    ],
)
connection.commit()
connection.close()
'@
    $CreateFixture | & $Python.Source - $DatabasePath
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $DatabasePath -PathType Leaf)) {
        throw 'failed to create the isolated SQLite performance fixture'
    }
}

function New-ZedRemoteFixture {
    param(
        [Parameter(Mandatory)]
        [string] $SampleDirectory
    )

    $Encoding = [Text.UTF8Encoding]::new($false)
    $GlobalStatePath = Join-Path $SampleDirectory '.codex-global-state.json'
    $RegistryPath = Join-Path $SampleDirectory 'zed-remote-projects.json'
    $LaunchRecordPath = Join-Path $SampleDirectory 'zed-launch-record.json'
    $SshUser = 'zed-perf-user-sentinel'
    $SshHost = 'zed-perf-host-sentinel.example.test'
    $SshPort = 2222
    $HostId = 'perf-zed-managed-host'
    $CurrentPath = '/zed/perf/current-path-sentinel'
    $ForgetPath = '/zed/perf/forget-path-sentinel'
    $StalePath = '/zed/perf/stale-path-sentinel'
    $CurrentUrl = "ssh://${SshUser}@${SshHost}:$SshPort$CurrentPath"
    $ForgetUrl = "ssh://${SshUser}@${SshHost}:$SshPort$ForgetPath"
    $StaleUrl = "ssh://${SshUser}@${SshHost}:$SshPort$StalePath"

    $GlobalState = [ordered]@{
        'selected-remote-host-id' = $HostId
        'codex-managed-remote-connections' = @(
            [ordered]@{
                hostId = $HostId
                hostname = "${SshUser}@${SshHost}"
                sshPort = $SshPort
            }
        )
        'remote-projects' = @(
            [ordered]@{
                id = 'perf-zed-current'
                hostId = $HostId
                remotePath = $CurrentPath
                label = 'Performance current Zed workspace'
            }
        )
        'project-order' = @('perf-zed-current')
    }
    [IO.File]::WriteAllText(
        $GlobalStatePath,
        ($GlobalState | ConvertTo-Json -Depth 8),
        $Encoding
    )

    $Registry = [ordered]@{
        projects = @(
            [ordered]@{
                id = 'perf-zed-forget'
                label = 'Performance forget fixture'
                hostId = $HostId
                ssh = [ordered]@{
                    user = $SshUser
                    host = $SshHost
                    port = $SshPort
                }
                path = $ForgetPath
                url = $ForgetUrl
                source = 'recent'
                lastOpenedAtMs = 2000
                isCurrent = $false
            },
            [ordered]@{
                id = 'perf-zed-stale'
                label = 'Performance stale fixture'
                hostId = $HostId
                ssh = [ordered]@{
                    user = $SshUser
                    host = $SshHost
                    port = $SshPort
                }
                path = $StalePath
                url = $StaleUrl
                source = 'recent'
                lastOpenedAtMs = 1000
                isCurrent = $false
            }
        )
        futureRegistry = [ordered]@{
            keep = $true
            label = 'preserved'
        }
    }
    [IO.File]::WriteAllText(
        $RegistryPath,
        ($Registry | ConvertTo-Json -Depth 8),
        $Encoding
    )

    [pscustomobject]@{
        GlobalStatePath = $GlobalStatePath
        RegistryPath = $RegistryPath
        LaunchRecordPath = $LaunchRecordPath
        CurrentPath = $CurrentPath
        ForgetPath = $ForgetPath
        StalePath = $StalePath
        SensitiveValues = @(
            $SshUser,
            $SshHost,
            $CurrentPath,
            $ForgetPath,
            $StalePath,
            $CurrentUrl,
            $ForgetUrl,
            $StaleUrl
        )
    }
}

function New-PendingImportFixture {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    $Pending = [ordered]@{
        name = 'Performance pending provider'
        baseUrl = 'https://pending.example.test/v1'
        apiKey = ''
        wireApi = 'responses'
        relayMode = 'pureApi'
        configContents = ''
        authContents = ''
    }
    $Json = $Pending | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText($Path, $Json, [Text.UTF8Encoding]::new($false))
}

function Start-ScriptMarketFixture {
    param(
        [Parameter(Mandatory)]
        [string] $SampleDirectory
    )

    $FixtureRoot = Join-Path $SampleDirectory 'user-script-fixture'
    $BuiltinDirectory = Join-Path $FixtureRoot 'builtin'
    $UserDirectory = Join-Path $FixtureRoot 'user'
    $MarketDirectory = Join-Path $FixtureRoot 'market'
    $ConfigPath = Join-Path $FixtureRoot 'user_scripts.json'
    New-Item -ItemType Directory -Path $BuiltinDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $UserDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $MarketDirectory -Force | Out-Null

    $Encoding = [Text.UTF8Encoding]::new($false)
    $BuiltinPath = Join-Path $BuiltinDirectory 'base.js'
    $CustomPath = Join-Path $UserDirectory 'custom.js'
    $UnrelatedPath = Join-Path $UserDirectory 'unrelated.js'
    $MarketScriptPath = Join-Path $MarketDirectory 'perf-script.js'
    [IO.File]::WriteAllText($BuiltinPath, 'builtin-performance-script', $Encoding)
    [IO.File]::WriteAllText($CustomPath, 'custom-performance-script', $Encoding)
    [IO.File]::WriteAllText($UnrelatedPath, 'unrelated-performance-script', $Encoding)
    [IO.File]::WriteAllText($MarketScriptPath, 'verified-performance-market-script', $Encoding)

    $Config = [ordered]@{
        enabled = $true
        scripts = [ordered]@{
            'user:custom.js' = $false
            'user:unrelated.js' = $true
        }
        market = [ordered]@{}
        futureRoot = [ordered]@{
            keep = $true
            label = 'preserved'
        }
    }
    [IO.File]::WriteAllText(
        $ConfigPath,
        ($Config | ConvertTo-Json -Depth 8),
        $Encoding
    )

    $Listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    try {
        $Listener.Start()
        $Port = ([Net.IPEndPoint]$Listener.LocalEndpoint).Port
    }
    finally {
        $Listener.Stop()
    }

    $PortText = $Port.ToString([Globalization.CultureInfo]::InvariantCulture)
    $IndexUrl = "http://127.0.0.1:$PortText/index.json"
    $ScriptUrl = "http://127.0.0.1:$PortText/perf-script.js"
    $Digest = (Get-FileHash -LiteralPath $MarketScriptPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $Manifest = [ordered]@{
        version = 1
        updated_at = '2026-07-18T00:00:00Z'
        scripts = @(
            [ordered]@{
                id = 'perf-script'
                name = 'Performance script'
                description = 'Keeps fixture metadata consistent.'
                version = '1'
                author = 'Performance fixture'
                tags = @('fixture')
                homepage = 'https://example.invalid/perf-script'
                script_url = $ScriptUrl
                sha256 = $Digest
            }
        )
    }
    [IO.File]::WriteAllText(
        (Join-Path $MarketDirectory 'index.json'),
        ($Manifest | ConvertTo-Json -Depth 8),
        $Encoding
    )

    $Python = Get-Command python -ErrorAction SilentlyContinue
    if ($null -eq $Python) {
        throw 'python is required to host the isolated user-script market fixture'
    }
    $AccessLogPath = Join-Path $FixtureRoot 'market-access.log'
    $ServerOutputPath = Join-Path $FixtureRoot 'market-output.log'
    $ServerProcess = $null
    try {
        $ServerProcess = Start-Process `
            -FilePath $Python.Source `
            -ArgumentList @('-m', 'http.server', $PortText, '--bind', '127.0.0.1') `
            -WorkingDirectory $MarketDirectory `
            -WindowStyle Hidden `
            -RedirectStandardOutput $ServerOutputPath `
            -RedirectStandardError $AccessLogPath `
            -PassThru

        $Deadline = [DateTime]::UtcNow.AddSeconds(5)
        $Ready = $false
        while ([DateTime]::UtcNow -lt $Deadline) {
            if ($ServerProcess.HasExited) {
                throw "user-script market fixture exited with code $($ServerProcess.ExitCode)"
            }
            try {
                $Response = Invoke-WebRequest -Uri $IndexUrl -UseBasicParsing -TimeoutSec 1
                if ($Response.StatusCode -eq 200) {
                    $Ready = $true
                    break
                }
            }
            catch {
                Start-Sleep -Milliseconds 50
            }
        }
        if (-not $Ready) {
            throw 'user-script market fixture did not become ready'
        }

        [pscustomobject]@{
            Process = $ServerProcess
            ProcessId = $ServerProcess.Id
            FixtureRoot = $FixtureRoot
            BuiltinDirectory = $BuiltinDirectory
            UserDirectory = $UserDirectory
            ConfigPath = $ConfigPath
            IndexUrl = $IndexUrl
            ScriptUrl = $ScriptUrl
            MarketScriptPath = $MarketScriptPath
            InstalledScriptPath = Join-Path $UserDirectory 'market-perf-script.js'
            BuiltinPath = $BuiltinPath
            CustomPath = $CustomPath
            UnrelatedPath = $UnrelatedPath
            AccessLogPath = $AccessLogPath
        }
    }
    catch {
        if ($null -ne $ServerProcess) {
            Stop-OwnedProcess -Process $ServerProcess
        }
        throw
    }
}

function Read-DiagnosticRecordsForProcess {
    param(
        [Parameter(Mandatory)]
        [string] $Path,

        [Parameter(Mandatory)]
        [long] $Offset,

        [Parameter(Mandatory)]
        [int] $ProcessId
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return ''
    }

    $Stream = [IO.File]::Open(
        $Path,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::ReadWrite
    )
    try {
        if ($Offset -gt $Stream.Length) {
            $Offset = 0
        }
        $null = $Stream.Seek($Offset, [IO.SeekOrigin]::Begin)
        $Reader = [IO.StreamReader]::new(
            $Stream,
            [Text.UTF8Encoding]::new($false),
            $true,
            1024,
            $true
        )
        try {
            $Text = $Reader.ReadToEnd()
        }
        finally {
            $Reader.Dispose()
        }
    }
    finally {
        $Stream.Dispose()
    }

    $Records = @()
    foreach ($Line in @($Text -split "`r?`n")) {
        if ([string]::IsNullOrWhiteSpace($Line)) {
            continue
        }
        try {
            $Record = $Line | ConvertFrom-Json
            if ([int]$Record.pid -eq $ProcessId) {
                $Records += ($Record | ConvertTo-Json -Depth 8 -Compress)
            }
        }
        catch {
            continue
        }
    }
    $Records -join "`n"
}

function Invoke-NativeSample {
    param(
        [Parameter(Mandatory)]
        [string] $Name,

        [Parameter(Mandatory)]
        [int] $ExitAfterMs,

        [int] $MemorySampleAfterSeconds = 0
    )

    $SampleDirectory = Join-Path $RunDirectory $Name
    $StateDirectory = Join-Path $SampleDirectory 'state'
    $ReportPath = Join-Path $SampleDirectory 'report.json'
    $SettingsPath = Join-Path $SampleDirectory 'settings.json'
    $CodexHome = Join-Path $SampleDirectory 'codex-home'
    $CcsDbPath = Join-Path $SampleDirectory 'cc-switch.db'
    $PendingImportPath = Join-Path $SampleDirectory 'pending-provider-import.json'
    $BackupDirectory = Join-Path $SampleDirectory 'backups'
    $ContextOwnershipPath = Join-Path $SampleDirectory 'context-live-ownership.json'
    New-Item -ItemType Directory -Path $SampleDirectory | Out-Null
    $MaintenanceSettingsFixture = New-MaintenanceSettingsFixture `
        -SampleDirectory $SampleDirectory
    $DiagnosticLogPath = $MaintenanceSettingsFixture.DiagnosticLogPath
    $ProviderSettingsFixture = New-ProviderSettingsFixture `
        -Path $SettingsPath `
        -MaintenanceSettingsFixture $MaintenanceSettingsFixture
    New-CodexHomeFixture -Path $CodexHome
    New-PendingImportFixture -Path $PendingImportPath
    $ZedFixture = New-ZedRemoteFixture -SampleDirectory $SampleDirectory
    $MarketFixture = $null
    $Process = $null
    $PrivateMemoryBytes = $null
    $DiagnosticStartOffset = if (Test-Path -LiteralPath $DiagnosticLogPath -PathType Leaf) {
        [long](Get-Item -LiteralPath $DiagnosticLogPath).Length
    }
    else {
        0L
    }

    try {
        $MarketFixture = Start-ScriptMarketFixture -SampleDirectory $SampleDirectory
        $env:CODEX_PLUS_NATIVE_STATE_DIR = $StateDirectory
        $env:CODEX_PLUS_NATIVE_PERF_REPORT = $ReportPath
        $env:CODEX_PLUS_NATIVE_PERF_EXIT_AFTER_MS = $ExitAfterMs.ToString(
            [Globalization.CultureInfo]::InvariantCulture
        )
        $env:CODEX_PLUS_NATIVE_SETTINGS_PATH = $SettingsPath
        $env:CODEX_PLUS_NATIVE_CODEX_HOME = $CodexHome
        $env:CODEX_PLUS_NATIVE_CCS_DB_PATH = $CcsDbPath
        $env:CODEX_PLUS_NATIVE_PENDING_IMPORT_PATH = $PendingImportPath
        $env:CODEX_PLUS_NATIVE_BACKUP_DIR = $BackupDirectory
        $env:CODEX_PLUS_NATIVE_CONTEXT_OWNERSHIP_PATH = $ContextOwnershipPath
        $env:CODEX_PLUS_NATIVE_USER_SCRIPT_BUILTIN_DIR = $MarketFixture.BuiltinDirectory
        $env:CODEX_PLUS_NATIVE_USER_SCRIPT_USER_DIR = $MarketFixture.UserDirectory
        $env:CODEX_PLUS_NATIVE_USER_SCRIPT_CONFIG_PATH = $MarketFixture.ConfigPath
        $env:CODEX_PLUS_SCRIPT_MARKET_INDEX_URL = $MarketFixture.IndexUrl
        $env:CODEX_PLUS_NATIVE_SCRIPT_MARKET_ALLOW_LOOPBACK = '1'
        $env:CODEX_PLUS_NATIVE_ZED_GLOBAL_STATE_PATH = $ZedFixture.GlobalStatePath
        $env:CODEX_PLUS_NATIVE_ZED_REGISTRY_PATH = $ZedFixture.RegistryPath
        $env:CODEX_PLUS_NATIVE_ZED_LAUNCH_RECORD_PATH = $ZedFixture.LaunchRecordPath
        $env:CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH = `
            $MaintenanceSettingsFixture.DiagnosticLogPath
        $env:CODEX_PLUS_NATIVE_LATEST_STATUS_PATH = `
            $MaintenanceSettingsFixture.LatestStatusPath
        $env:CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_FIXTURE_STATE = `
            'windows_needs_repair_legacy'
        $env:CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_RECORD_PATH = `
            $MaintenanceSettingsFixture.DesktopIntegrationRecordPath
        $env:CODEX_PLUS_NATIVE_ENTRYPOINT_SILENT_INSTALLED = '1'
        $env:CODEX_PLUS_NATIVE_ENTRYPOINT_MANAGEMENT_INSTALLED = '0'
        $env:CODEX_PLUS_NATIVE_CODEX_LAUNCH_RECORD_PATH = `
            $MaintenanceSettingsFixture.CodexLaunchRecordPath
        $env:CODEX_PLUS_NATIVE_PATH_PICKER_RESPONSES_PATH = `
            $MaintenanceSettingsFixture.PathPickerResponsesPath
        $env:CODEX_PLUS_NATIVE_PATH_PICKER_RECORD_PATH = `
            $MaintenanceSettingsFixture.PathPickerRecordPath
        $env:CODEX_PLUS_NATIVE_STEPWISE_TEST_RECORD_PATH = `
            $MaintenanceSettingsFixture.StepwiseRecordPath
        $env:CODEX_PLUS_NATIVE_STEPWISE_TEST_RESULT = `
            $MaintenanceSettingsFixture.StepwiseResult
        $env:CODEX_PLUS_NATIVE_UPDATE_METADATA_PATH = `
            $MaintenanceSettingsFixture.UpdateMetadataPath
        $env:CODEX_PLUS_NATIVE_UPDATE_ASSET_PATH = `
            $MaintenanceSettingsFixture.UpdateAssetPath
        $env:CODEX_PLUS_NATIVE_UPDATE_LAUNCH_RECORD_PATH = `
            $MaintenanceSettingsFixture.UpdateLaunchRecordPath
        $env:CODEX_PLUS_NATIVE_UPDATE_CHECK_RECORD_PATH = `
            $MaintenanceSettingsFixture.UpdateCheckRecordPath
        $env:CODEX_PLUS_NATIVE_ENV_PROCESS_ONLY = '1'
        $env:OPENAI_CODEX_PLUS_PERF_SENTINEL = 'present'

        Assert-MaintenanceSettingsFixtureSetup `
            -Fixture $MaintenanceSettingsFixture `
            -SampleDirectory $SampleDirectory `
            -SettingsPath $SettingsPath

        $Process = Start-Process `
            -FilePath $BinaryPath `
            -WorkingDirectory $RepositoryRoot `
            -WindowStyle Hidden `
            -PassThru

        if ($MemorySampleAfterSeconds -gt 0) {
            Start-Sleep -Seconds $MemorySampleAfterSeconds
            $LiveProcess = Get-Process -Id $Process.Id -ErrorAction Stop
            $LiveProcess.Refresh()
            $PrivateMemoryBytes = [long] $LiveProcess.PrivateMemorySize64
        }

        $RemainingWaitMs = [Math]::Max(
            15000,
            $ExitAfterMs + 10000 - ($MemorySampleAfterSeconds * 1000)
        )
        if (-not $Process.WaitForExit($RemainingWaitMs)) {
            throw "$Name did not exit within the expected interval"
        }
        if ($Process.ExitCode -ne 0) {
            throw "$Name exited with code $($Process.ExitCode)"
        }
        $UpdateChecks = @(Get-Content -LiteralPath `
            $MaintenanceSettingsFixture.UpdateCheckRecordPath)
        if ($UpdateChecks.Count -ne 1 -or $UpdateChecks[0] -ne 'check') {
            throw "$Name did not perform exactly one isolated startup update check"
        }
        if (Test-Path -LiteralPath $MaintenanceSettingsFixture.UpdateLaunchRecordPath) {
            throw "$Name unexpectedly launched the update fixture"
        }

        $Report = Wait-ForReport -Path $ReportPath
        $ReportText = Get-Content -LiteralPath $ReportPath -Raw
        $DiagnosticText = Read-DiagnosticRecordsForProcess `
            -Path $DiagnosticLogPath `
            -Offset $DiagnosticStartOffset `
            -ProcessId $Process.Id
        if ($null -eq $Report.first_ui_frame_ms) {
            throw "$Name did not record first_ui_frame_ms"
        }

        [pscustomobject]@{
            Name = $Name
            FirstUiFrameMs = [double] $Report.first_ui_frame_ms
            OverviewReadyMs = if ($null -eq $Report.overview_ready_ms) {
                $null
            }
            else {
                [double] $Report.overview_ready_ms
            }
            CpuSamplesMs = @($Report.cpu_frame_samples_ms | ForEach-Object { [double] $_ })
            InputSamplesMs = @($Report.input_latency_samples_ms | ForEach-Object { [double] $_ })
            ScriptActions = @($Report.script_actions | ForEach-Object { [string] $_ })
            PrivateMemoryBytes = $PrivateMemoryBytes
            ReportPath = $ReportPath
            ReportText = $ReportText
            DiagnosticText = $DiagnosticText
            SettingsPath = $SettingsPath
            CodexHomePath = $CodexHome
            LiveConfigPath = Join-Path $CodexHome 'config.toml'
            ContextOwnershipPath = $ContextOwnershipPath
            SampleDirectory = $SampleDirectory
            BackupDirectory = $BackupDirectory
            UserScriptFixtureRoot = $MarketFixture.FixtureRoot
            UserScriptBuiltinDirectory = $MarketFixture.BuiltinDirectory
            UserScriptUserDirectory = $MarketFixture.UserDirectory
            UserScriptConfigPath = $MarketFixture.ConfigPath
            UserScriptMarketIndexUrl = $MarketFixture.IndexUrl
            UserScriptMarketScriptUrl = $MarketFixture.ScriptUrl
            UserScriptMarketSourcePath = $MarketFixture.MarketScriptPath
            UserScriptInstalledPath = $MarketFixture.InstalledScriptPath
            UserScriptBuiltinPath = $MarketFixture.BuiltinPath
            UserScriptCustomPath = $MarketFixture.CustomPath
            UserScriptUnrelatedPath = $MarketFixture.UnrelatedPath
            UserScriptMarketAccessLogPath = $MarketFixture.AccessLogPath
            UserScriptMarketProcessId = $MarketFixture.ProcessId
            ZedGlobalStatePath = $ZedFixture.GlobalStatePath
            ZedRegistryPath = $ZedFixture.RegistryPath
            ZedLaunchRecordPath = $ZedFixture.LaunchRecordPath
            ZedCurrentPath = $ZedFixture.CurrentPath
            ZedForgetPath = $ZedFixture.ForgetPath
            ZedStalePath = $ZedFixture.StalePath
            ZedSensitiveValues = $ZedFixture.SensitiveValues
            MaintenanceSettingsFixture = $MaintenanceSettingsFixture
            ProviderSettingsFixture = $ProviderSettingsFixture
        }
    }
    finally {
        if ($null -ne $Process) {
            Stop-OwnedProcess -Process $Process
        }
        if ($null -ne $MarketFixture) {
            Stop-OwnedProcess -Process $MarketFixture.Process
        }
    }
}

function Assert-MaintenanceSettingsWorkflowResult {
    param(
        [Parameter(Mandatory)]
        [pscustomobject] $Sample
    )

    $Fixture = $Sample.MaintenanceSettingsFixture
    $Settings = Get-Content -LiteralPath $Sample.SettingsPath -Raw | ConvertFrom-Json
    if ($Settings.codexAppPath -ne $Fixture.AppPath) {
        throw 'the maintenance workflow did not persist the scripted app path'
    }
    if (
        $Settings.codexAppStepwiseEnabled -ne $true -or
        $Settings.codexAppStepwiseDirectSend -ne $true -or
        $Settings.codexAppStepwiseBaseUrl -ne $Fixture.SavedStepwiseUrl -or
        $Settings.codexAppStepwiseApiKey -ne $Fixture.SecretSentinel -or
        $Settings.codexAppStepwiseApiKeyEnv -ne $Fixture.SavedStepwiseEnvironment -or
        $Settings.codexAppStepwiseModel -ne $Fixture.SavedStepwiseModel -or
        [int]$Settings.codexAppStepwiseMaxItems -ne 5 -or
        [int]$Settings.codexAppStepwiseMaxInputChars -ne 7000 -or
        [int]$Settings.codexAppStepwiseMaxOutputTokens -ne 700 -or
        [int64]$Settings.codexAppStepwiseTimeoutMs -ne 9000
    ) {
        throw 'the Stepwise workflow did not persist the complete scripted group'
    }
    if (
        $Settings.codexAppImageOverlayEnabled -ne $true -or
        $Settings.codexAppImageOverlayPath -ne $Fixture.SelectedImagePath -or
        [int]$Settings.codexAppImageOverlayOpacity -ne 41 -or
        $Settings.codexAppImageOverlayFitMode -ne 'fill'
    ) {
        throw 'the image-overlay workflow did not persist the complete scripted group'
    }
    if ((@($Settings.codexExtraArgs) -join "`n") -ne ($Fixture.SavedExtraArgs -join "`n")) {
        throw 'the launch-arguments workflow did not persist the scripted values'
    }
    if ($Settings.enhancementsEnabled -ne $false) {
        throw 'the enhancements workflow did not persist the scripted master setting'
    }
    if (
        $Settings.futureSettingsRoot.keep -ne $true -or
        $Settings.futureSettingsRoot.label -ne 'maintenance-settings-preserved' -or
        $Settings.activeRelayId -ne 'perf-provider-a' -or
        @($Settings.relayProfiles).Count -ne 2 -or
        $Settings.relayProfiles[0].name -ne 'Performance provider A' -or
        $Settings.relayProfiles[1].name -ne 'Performance provider B'
    ) {
        throw 'the settings workflow did not preserve unknown or unrelated provider fields'
    }
    for ($Index = 0; $Index -lt $Sample.ProviderSettingsFixture.RelayProfiles.Count; $Index++) {
        $ExpectedProfile = $Sample.ProviderSettingsFixture.RelayProfiles[$Index]
        $ActualProfile = $Settings.relayProfiles[$Index]
        foreach ($Property in $ExpectedProfile.PSObject.Properties.Name) {
            $ExpectedValue = ConvertTo-Json -InputObject $ExpectedProfile.$Property -Depth 8 -Compress
            $ActualValue = ConvertTo-Json -InputObject $ActualProfile.$Property -Depth 8 -Compress
            if ($ActualValue -cne $ExpectedValue) {
                throw 'the settings workflow changed an unrelated provider field'
            }
        }
    }

    if (-not (Test-Path -LiteralPath $Fixture.PathPickerRecordPath -PathType Leaf)) {
        throw 'the isolated path picker did not write a record'
    }
    $PickerText = Get-Content -LiteralPath $Fixture.PathPickerRecordPath -Raw
    $PickerRecords = $PickerText | ConvertFrom-Json
    if (
        $PickerRecords.Count -ne 2 -or
        $PickerRecords[0].target -ne 'maintenance_executable' -or
        $PickerRecords[0].selected -ne $true -or
        $PickerRecords[0].cancelled -ne $false -or
        $PickerRecords[1].target -ne 'settings_overlay_image' -or
        $PickerRecords[1].selected -ne $true -or
        $PickerRecords[1].cancelled -ne $false
    ) {
        throw 'the isolated path picker did not record executable and image targets in order'
    }
    foreach ($Record in $PickerRecords) {
        if ((@($Record.PSObject.Properties.Name | Sort-Object) -join ',') -ne 'cancelled,selected,target') {
            throw 'the path-picker record disclosed data beyond safe ordered metadata'
        }
    }

    if (-not (Test-Path -LiteralPath $Fixture.StepwiseRecordPath -PathType Leaf)) {
        throw 'the recording Stepwise tester did not write a record'
    }
    $StepwiseText = Get-Content -LiteralPath $Fixture.StepwiseRecordPath -Raw
    $Stepwise = $StepwiseText | ConvertFrom-Json
    $ExpectedStepwiseFields = @(
        'baseUrlConfigured',
        'callCount',
        'directKeyConfigured',
        'enabled',
        'environmentNameConfigured',
        'maxInputChars',
        'maxItems',
        'maxOutputTokens',
        'modelLength',
        'operation',
        'timeoutMs'
    ) | Sort-Object
    if (
        (@($Stepwise.PSObject.Properties.Name | Sort-Object) -join ',') -ne
            ($ExpectedStepwiseFields -join ',') -or
        $Stepwise.operation -ne 'stepwise_test' -or
        [int]$Stepwise.callCount -ne 1 -or
        $Stepwise.enabled -ne $true -or
        $Stepwise.baseUrlConfigured -ne $true -or
        $Stepwise.directKeyConfigured -ne $true -or
        $Stepwise.environmentNameConfigured -ne $true -or
        [int]$Stepwise.modelLength -ne $Fixture.SavedStepwiseModel.Length -or
        [int]$Stepwise.maxItems -ne 5 -or
        [int]$Stepwise.maxInputChars -ne 7000 -or
        [int]$Stepwise.maxOutputTokens -ne 700 -or
        [int64]$Stepwise.timeoutMs -ne 9000
    ) {
        throw 'the recording Stepwise tester captured an unexpected or unsafe call'
    }

    if (-not (Test-Path -LiteralPath $Fixture.CodexLaunchRecordPath -PathType Leaf)) {
        throw 'the recording Codex launcher did not write a record'
    }
    $LaunchText = Get-Content -LiteralPath $Fixture.CodexLaunchRecordPath -Raw
    $Launch = $LaunchText | ConvertFrom-Json
    if (
        (@($Launch.PSObject.Properties.Name | Sort-Object) -join ',') -ne
            'argumentCount,callCount,debugPort,helperPort,operation,pathConfigured' -or
        $Launch.operation -ne 'launch' -or
        [int]$Launch.callCount -ne 1 -or
        [int]$Launch.debugPort -ne 9229 -or
        [int]$Launch.helperPort -ne 57321 -or
        $Launch.pathConfigured -ne $true -or
        [int]$Launch.argumentCount -ne 6
    ) {
        throw 'the recording Codex launcher captured an unexpected request'
    }

    if (-not (Test-Path -LiteralPath $Fixture.DesktopIntegrationRecordPath -PathType Leaf)) {
        throw 'the recording desktop-integration fixture did not write a record'
    }
    $DesktopIntegrationText = Get-Content `
        -LiteralPath $Fixture.DesktopIntegrationRecordPath `
        -Raw
    $DesktopIntegrationOperations = @(
        $DesktopIntegrationText -split "`r?`n" | Where-Object { $_ -ne '' }
    )
    $ExpectedDesktopIntegrationOperations = @(
        'repair:desktop_manager_shortcut',
        'repair:start_menu_launcher_shortcut',
        'repair:start_menu_manager_shortcut',
        'repair:url_protocol',
        'startup:set_canonical',
        'startup:delete_legacy_run',
        'startup:delete_canonical',
        'startup:set_canonical'
    )
    if (
        $DesktopIntegrationOperations.Count -ne 8 -or
        ($DesktopIntegrationOperations -join "`n") -cne
            ($ExpectedDesktopIntegrationOperations -join "`n")
    ) {
        throw 'the desktop-integration workflow did not record the exact bounded operation order'
    }

    $SensitiveValues = @(
        $Fixture.SecretSentinel,
        'private-path-sentinel',
        $Fixture.InitialStepwiseUrl,
        $Fixture.SavedStepwiseUrl,
        $Fixture.InitialStepwiseModel,
        $Fixture.SavedStepwiseModel,
        'private-body-sentinel'
    )
    foreach ($Sensitive in $SensitiveValues) {
        if ($Sample.ReportText.Contains([string]$Sensitive)) {
            throw 'the performance report disclosed a key, path, URL, model, or body sentinel'
        }
        if ($StepwiseText.Contains([string]$Sensitive)) {
            throw 'the Stepwise record disclosed a key, URL, or model sentinel'
        }
        if ($LaunchText.Contains([string]$Sensitive)) {
            throw 'the Codex launch record disclosed a path or argument sentinel'
        }
        if ($PickerText.Contains([string]$Sensitive)) {
            throw 'the path-picker record disclosed a selected path sentinel'
        }
        if ($DesktopIntegrationText.Contains([string]$Sensitive)) {
            throw 'the desktop-integration record disclosed private fixture data'
        }
    }
    if ($Sample.ReportText.Contains('native_manager.perf_fixture')) {
        throw 'the raw maintenance diagnostic fixture was copied into performance evidence'
    }

    if (
        (Get-Content -LiteralPath $Fixture.LegacyWatcherSentinelPath -Raw).Trim() -ne
            'disabled'
    ) {
        throw 'the maintenance workflow mutated the unrelated legacy Watcher sentinel'
    }
    foreach ($UnexpectedArtifact in @(
        $Fixture.EntrypointMutationPath,
        $Fixture.WatcherMutationPath,
        $Fixture.RealLaunchArtifactPath
    )) {
        if (Test-Path -LiteralPath $UnexpectedArtifact) {
            throw "the maintenance workflow created a forbidden mutation artifact: $UnexpectedArtifact"
        }
    }
}

function Assert-ContextWorkflowResult {
    param(
        [Parameter(Mandatory)]
        [pscustomobject] $Sample
    )

    $Settings = Get-Content -LiteralPath $Sample.SettingsPath -Raw | ConvertFrom-Json
    if ($Settings.relayContextConfigContents -notmatch '(?ms)^\[skills\.review\].*?^enabled\s*=\s*false\s*$') {
        throw 'the real-window context toggle did not persist the disabled skill'
    }

    $LiveConfig = Get-Content -LiteralPath $Sample.LiveConfigPath -Raw
    if ($LiveConfig -notmatch '(?m)^\[mcp_servers\.alpha\]\s*$') {
        throw 'the real-window context sync did not install mcp:alpha'
    }
    if ($LiveConfig -notmatch '(?m)^\[plugins\.lint\]\s*$') {
        throw 'the real-window context sync did not install plugin:lint'
    }
    if ($LiveConfig -match '(?m)^\[skills\.review\]\s*$') {
        throw 'the real-window context sync installed a disabled skill'
    }

    if (-not (Test-Path -LiteralPath $Sample.ContextOwnershipPath -PathType Leaf)) {
        throw 'the real-window context sync did not write an ownership manifest'
    }
    $Ownership = Get-Content -LiteralPath $Sample.ContextOwnershipPath -Raw | ConvertFrom-Json
    $OwnedKeys = @($Ownership.entries | ForEach-Object {
        "$($_.identity.kind):$($_.identity.id)"
    })
    if (($OwnedKeys -join ',') -ne 'mcp:alpha,plugin:lint') {
        throw "unexpected real-window context ownership keys: $($OwnedKeys -join ',')"
    }
    foreach ($Entry in @($Ownership.entries)) {
        if ([string]$Entry.bodySha256 -notmatch '^[0-9a-f]{64}$') {
            throw 'the context ownership manifest contains an invalid body hash'
        }
        if ((@($Entry.PSObject.Properties.Name | Sort-Object) -join ',') -ne 'bodySha256,identity') {
            throw 'the context ownership manifest contains unexpected entry fields'
        }
    }
}

function Assert-MarketplaceWorkflowResult {
    param(
        [Parameter(Mandatory)]
        [pscustomobject] $Sample
    )

    $Config = Get-Content -LiteralPath $Sample.LiveConfigPath -Raw
    foreach ($Marketplace in @(
        'openai-curated',
        'openai-api-curated',
        'openai-curated-remote'
    )) {
        $Escaped = [Regex]::Escape($Marketplace)
        if ($Config -notmatch "(?m)^\[marketplaces\.$Escaped\]\s*$") {
            throw "the real-window marketplace workflow did not register $Marketplace"
        }
    }
    if ($Config -notmatch '(?m)^model\s*=\s*"perf-model"\s*$') {
        throw 'the marketplace workflow did not preserve the existing model setting'
    }
    if ($Config -notmatch '(?m)^\[mcp_servers\.alpha\]\s*$') {
        throw 'the marketplace workflow did not preserve the synced MCP table'
    }
    if ($Config -notmatch '(?m)^\[plugins\.lint\]\s*$') {
        throw 'the marketplace workflow did not preserve the synced plugin table'
    }

    $LocalRoot = Join-Path $Sample.CodexHomePath '.tmp\plugins'
    $RemoteRoot = Join-Path $Sample.CodexHomePath '.tmp\plugins-remote'
    $LocalManifestPath = Join-Path $LocalRoot '.agents\plugins\marketplace.json'
    $RemoteManifestPath = Join-Path $RemoteRoot '.agents\plugins\marketplace.json'
    foreach ($ManifestPath in @($LocalManifestPath, $RemoteManifestPath)) {
        if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
            throw "marketplace manifest was not persisted: $ManifestPath"
        }
    }

    $LocalManifest = Get-Content -LiteralPath $LocalManifestPath -Raw | ConvertFrom-Json
    $RemoteManifest = Get-Content -LiteralPath $RemoteManifestPath -Raw | ConvertFrom-Json
    $LocalPluginCount = @($LocalManifest.plugins).Count
    $RemotePluginCount = @($RemoteManifest.plugins).Count
    $LocalSkillCount = @(Get-ChildItem `
        -LiteralPath (Join-Path $LocalRoot 'plugins') `
        -Filter 'SKILL.md' `
        -File `
        -Recurse).Count
    $RemoteSkillCount = @(Get-ChildItem `
        -LiteralPath (Join-Path $RemoteRoot 'plugins') `
        -Filter 'SKILL.md' `
        -File `
        -Recurse).Count
    if ($LocalPluginCount -ne 1 -or $LocalSkillCount -ne 1) {
        throw "unexpected local marketplace counts: plugins=$LocalPluginCount skills=$LocalSkillCount"
    }
    if ($RemotePluginCount -ne 10 -or $RemoteSkillCount -ne 110) {
        throw "unexpected remote marketplace counts: plugins=$RemotePluginCount skills=$RemoteSkillCount"
    }

    $Staging = @(Get-ChildItem `
        -LiteralPath (Join-Path $Sample.CodexHomePath '.tmp') `
        -Directory | Where-Object {
            $_.Name -like 'plugins-download-*' -or
            $_.Name -like 'plugins-remote-embedded-*'
        })
    if ($Staging.Count -ne 0) {
        throw "marketplace staging directories were not cleaned: $($Staging.Name -join ',')"
    }
}

function Assert-UserScriptWorkflowResult {
    param(
        [Parameter(Mandatory)]
        [pscustomobject] $Sample
    )

    $SampleRoot = [IO.Path]::GetFullPath($Sample.SampleDirectory).TrimEnd('\') + '\'
    foreach ($FixturePath in @(
        $Sample.UserScriptFixtureRoot,
        $Sample.UserScriptBuiltinDirectory,
        $Sample.UserScriptUserDirectory,
        $Sample.UserScriptConfigPath,
        $Sample.BackupDirectory
    )) {
        $Resolved = [IO.Path]::GetFullPath([string]$FixturePath)
        if (-not $Resolved.StartsWith($SampleRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "user-script fixture escaped the isolated sample root: $Resolved"
        }
    }

    $IndexUri = [Uri]$Sample.UserScriptMarketIndexUrl
    if ($IndexUri.Scheme -ne 'http' -or $IndexUri.Host -ne '127.0.0.1') {
        throw 'the user-script market did not use the isolated loopback endpoint'
    }
    if (Get-Process -Id $Sample.UserScriptMarketProcessId -ErrorAction SilentlyContinue) {
        throw 'the owned user-script market fixture process was not terminated'
    }

    $Config = Get-Content -LiteralPath $Sample.UserScriptConfigPath -Raw | ConvertFrom-Json
    if ($Config.enabled -ne $false) {
        throw 'the user-script master switch did not remain disabled after the stale conflict'
    }
    if ($Config.futureRoot.keep -ne $true -or $Config.futureRoot.label -ne 'preserved') {
        throw 'the user-script workflow did not preserve unknown root configuration'
    }
    if ($null -ne $Config.scripts.PSObject.Properties['user:custom.js']) {
        throw 'the selected custom user script still has a configuration entry after deletion'
    }
    foreach ($Key in @('user:market-perf-script.js', 'user:unrelated.js')) {
        $Choice = $Config.scripts.PSObject.Properties[$Key]
        if ($null -eq $Choice -or $Choice.Value -ne $true) {
            throw "the user-script workflow changed an unrelated choice: $Key"
        }
    }

    $InstallProperty = $Config.market.PSObject.Properties['user:market-perf-script.js']
    if ($null -eq $InstallProperty) {
        throw 'the verified market install did not write metadata'
    }
    $Install = $InstallProperty.Value
    if (
        $Install.id -ne 'perf-script' -or
        $Install.name -ne 'Performance script' -or
        $Install.version -ne '1' -or
        $Install.script_url -ne $Sample.UserScriptMarketScriptUrl -or
        [string]::IsNullOrWhiteSpace([string]$Install.installed_at)
    ) {
        throw 'the verified market install metadata is incomplete or unexpected'
    }

    if (-not (Test-Path -LiteralPath $Sample.UserScriptInstalledPath -PathType Leaf)) {
        throw 'the verified market script was not installed'
    }
    $InstalledBytes = [Convert]::ToBase64String(
        [IO.File]::ReadAllBytes($Sample.UserScriptInstalledPath)
    )
    $MarketBytes = [Convert]::ToBase64String(
        [IO.File]::ReadAllBytes($Sample.UserScriptMarketSourcePath)
    )
    if ($InstalledBytes -ne $MarketBytes) {
        throw 'the verified market install bytes do not match the checked fixture bytes'
    }
    if (Test-Path -LiteralPath $Sample.UserScriptCustomPath) {
        throw 'the selected custom user script was not deleted'
    }
    foreach ($PreservedPath in @($Sample.UserScriptBuiltinPath, $Sample.UserScriptUnrelatedPath)) {
        if (-not (Test-Path -LiteralPath $PreservedPath -PathType Leaf)) {
            throw "the user-script workflow removed an unrelated script: $PreservedPath"
        }
    }
    $UserFiles = @(Get-ChildItem -LiteralPath $Sample.UserScriptUserDirectory -File |
        Sort-Object Name | ForEach-Object { $_.Name })
    if (($UserFiles -join ',') -ne 'market-perf-script.js,unrelated.js') {
        throw "unexpected user-script files after deletion: $($UserFiles -join ',')"
    }

    $BackupRoot = Join-Path $Sample.BackupDirectory 'user-scripts'
    if (-not (Test-Path -LiteralPath $BackupRoot -PathType Container)) {
        throw 'the user-script deletion did not create a backup root'
    }
    $DeleteBackups = @()
    foreach ($Directory in @(Get-ChildItem -LiteralPath $BackupRoot -Directory)) {
        $MetadataPath = Join-Path $Directory.FullName 'metadata.json'
        if (-not (Test-Path -LiteralPath $MetadataPath -PathType Leaf)) {
            continue
        }
        $Metadata = Get-Content -LiteralPath $MetadataPath -Raw | ConvertFrom-Json
        if ($Metadata.operation -eq 'delete' -and $Metadata.key -eq 'user:custom.js') {
            $DeleteBackups += [pscustomobject]@{
                Directory = $Directory.FullName
                Metadata = $Metadata
            }
        }
    }
    if ($DeleteBackups.Count -ne 1) {
        throw "expected one exact custom-script delete backup, got $($DeleteBackups.Count)"
    }
    $DeleteBackup = $DeleteBackups[0]
    if (
        $DeleteBackup.Metadata.schema -ne 1 -or
        $DeleteBackup.Metadata.file_name -ne 'custom.js' -or
        $DeleteBackup.Metadata.script_choice -ne $true -or
        $null -ne $DeleteBackup.Metadata.market_entry
    ) {
        throw 'the custom-script delete backup metadata is incomplete or unexpected'
    }
    $BackupScriptPath = Join-Path $DeleteBackup.Directory 'script.js'
    if (-not (Test-Path -LiteralPath $BackupScriptPath -PathType Leaf)) {
        throw 'the custom-script delete backup did not contain recoverable bytes'
    }
    $BackupText = [Text.UTF8Encoding]::new($false).GetString(
        [IO.File]::ReadAllBytes($BackupScriptPath)
    )
    if ($BackupText -ne 'custom-performance-script') {
        throw 'the custom-script delete backup bytes are not recoverable'
    }

    $AccessLog = Get-Content -LiteralPath $Sample.UserScriptMarketAccessLogPath -Raw
    $Requests = @([Regex]::Matches($AccessLog, '"GET ([^ ]+) HTTP/') | ForEach-Object {
        $_.Groups[1].Value
    })
    if ($Requests.Count -eq 0) {
        throw 'the isolated user-script market fixture did not receive any requests'
    }
    foreach ($RequestPath in $Requests) {
        if ($RequestPath -notin @('/index.json', '/perf-script.js')) {
            throw "unexpected user-script market request path: $RequestPath"
        }
    }
}

function Assert-ZedRemoteWorkflowResult {
    param(
        [Parameter(Mandatory)]
        [pscustomobject] $Sample
    )

    $SampleRoot = [IO.Path]::GetFullPath($Sample.SampleDirectory).TrimEnd('\') + '\'
    foreach ($FixturePath in @(
        $Sample.ZedGlobalStatePath,
        $Sample.ZedRegistryPath,
        $Sample.ZedLaunchRecordPath
    )) {
        $Resolved = [IO.Path]::GetFullPath([string]$FixturePath)
        if (-not $Resolved.StartsWith($SampleRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Zed fixture escaped the isolated sample root: $Resolved"
        }
    }

    $Settings = Get-Content -LiteralPath $Sample.SettingsPath -Raw | ConvertFrom-Json
    if ($Settings.zedRemoteOpenStrategy -ne 'newWindow') {
        throw 'the Zed workflow did not persist the new-window strategy'
    }
    if ($Settings.zedRemoteProjectRegistryEnabled -ne $true) {
        throw 'the Zed workflow did not persist the enabled project registry'
    }
    if (
        $Settings.futureZedRoot.keep -ne $true -or
        $Settings.futureZedRoot.label -ne 'preserved' -or
        $Settings.activeRelayId -ne 'perf-provider-a'
    ) {
        throw 'the Zed preference save did not preserve unrelated settings'
    }

    if (-not (Test-Path -LiteralPath $Sample.ZedLaunchRecordPath -PathType Leaf)) {
        throw 'the recording Zed launcher did not write a launch record'
    }
    $Launch = Get-Content -LiteralPath $Sample.ZedLaunchRecordPath -Raw | ConvertFrom-Json
    if ($Launch.strategy -ne 'newWindow' -or [int]$Launch.argumentCount -ne 2) {
        throw 'the recording Zed launcher captured unexpected launch arguments'
    }
    if ((@($Launch.PSObject.Properties.Name | Sort-Object) -join ',') -ne 'argumentCount,strategy') {
        throw 'the recording Zed launcher disclosed data beyond strategy and argument count'
    }

    $Registry = Get-Content -LiteralPath $Sample.ZedRegistryPath -Raw | ConvertFrom-Json
    if ($Registry.futureRegistry.keep -ne $true -or $Registry.futureRegistry.label -ne 'preserved') {
        throw 'the Zed registry mutation did not preserve unknown root fields'
    }
    $Projects = @($Registry.projects)
    $Remembered = @($Projects | Where-Object { $_.path -eq $Sample.ZedCurrentPath })
    if (
        $Remembered.Count -ne 1 -or
        $Remembered[0].source -ne 'recent' -or
        $Remembered[0].isCurrent -ne $false
    ) {
        throw 'the confirmed Zed launch did not remember the current project exactly once'
    }
    if (@($Projects | Where-Object { $_.path -eq $Sample.ZedForgetPath }).Count -ne 0) {
        throw 'the confirmed Zed forget did not remove its selected fixture'
    }
    if (@($Projects | Where-Object { $_.path -eq $Sample.ZedStalePath }).Count -ne 1) {
        throw 'the stale Zed forget unexpectedly changed the registry'
    }

    $ConflictRecorded = $false
    foreach ($Line in @($Sample.DiagnosticText -split "`r?`n")) {
        if ([string]::IsNullOrWhiteSpace($Line)) {
            continue
        }
        $Record = $Line | ConvertFrom-Json
        if (
            $Record.event -eq 'native_manager.zed_remote_failed' -and
            $Record.detail.operation -eq 'forget' -and
            $Record.detail.kind -eq 'RegistryConflict'
        ) {
            $ConflictRecorded = $true
        }
    }
    if (-not $ConflictRecorded) {
        throw 'the Zed workflow did not exercise the registry-conflict refresh path'
    }

    $LaunchText = Get-Content -LiteralPath $Sample.ZedLaunchRecordPath -Raw
    foreach ($Sensitive in @($Sample.ZedSensitiveValues)) {
        if ($Sample.ReportText.Contains([string]$Sensitive)) {
            throw 'the performance report disclosed a Zed URL, SSH identity, or remote path'
        }
        if ($Sample.DiagnosticText.Contains([string]$Sensitive)) {
            throw 'the diagnostic log disclosed a Zed URL, SSH identity, or remote path'
        }
        if ($LaunchText.Contains([string]$Sensitive)) {
            throw 'the recording launcher disclosed a Zed URL, SSH identity, or remote path'
        }
    }
}

function Get-Percentile {
    param(
        [Parameter(Mandatory)]
        [double[]] $Values,

        [Parameter(Mandatory)]
        [double] $Percentile
    )

    if ($Values.Count -eq 0) {
        return $null
    }
    $Sorted = @($Values | Sort-Object)
    $Index = [Math]::Ceiling($Sorted.Count * $Percentile) - 1
    $Index = [Math]::Max(0, [Math]::Min($Sorted.Count - 1, $Index))
    [double] $Sorted[$Index]
}

function Get-Maximum {
    param(
        [Parameter(Mandatory)]
        [double[]] $Values
    )

    if ($Values.Count -eq 0) {
        return $null
    }
    [double] (($Values | Measure-Object -Maximum).Maximum)
}

$PreviousEnvironment = @{}
foreach ($Name in @(
    'CODEX_PLUS_NATIVE_STATE_DIR',
    'CODEX_PLUS_NATIVE_PERF_REPORT',
    'CODEX_PLUS_NATIVE_PERF_EXIT_AFTER_MS',
    'CODEX_PLUS_NATIVE_SETTINGS_PATH',
    'CODEX_PLUS_NATIVE_CODEX_HOME',
    'CODEX_PLUS_NATIVE_CCS_DB_PATH',
    'CODEX_PLUS_NATIVE_PENDING_IMPORT_PATH',
    'CODEX_PLUS_NATIVE_BACKUP_DIR',
    'CODEX_PLUS_NATIVE_CONTEXT_OWNERSHIP_PATH',
    'CODEX_PLUS_NATIVE_USER_SCRIPT_BUILTIN_DIR',
    'CODEX_PLUS_NATIVE_USER_SCRIPT_USER_DIR',
    'CODEX_PLUS_NATIVE_USER_SCRIPT_CONFIG_PATH',
    'CODEX_PLUS_SCRIPT_MARKET_INDEX_URL',
    'CODEX_PLUS_NATIVE_SCRIPT_MARKET_ALLOW_LOOPBACK',
    'CODEX_PLUS_NATIVE_ZED_GLOBAL_STATE_PATH',
    'CODEX_PLUS_NATIVE_ZED_REGISTRY_PATH',
    'CODEX_PLUS_NATIVE_ZED_LAUNCH_RECORD_PATH',
    'CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH',
    'CODEX_PLUS_NATIVE_LATEST_STATUS_PATH',
    'CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_FIXTURE_STATE',
    'CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_RECORD_PATH',
    'CODEX_PLUS_NATIVE_ENTRYPOINT_SILENT_INSTALLED',
    'CODEX_PLUS_NATIVE_ENTRYPOINT_MANAGEMENT_INSTALLED',
    'CODEX_PLUS_NATIVE_CODEX_LAUNCH_RECORD_PATH',
    'CODEX_PLUS_NATIVE_PATH_PICKER_RESPONSES_PATH',
    'CODEX_PLUS_NATIVE_PATH_PICKER_RECORD_PATH',
    'CODEX_PLUS_NATIVE_STEPWISE_TEST_RECORD_PATH',
    'CODEX_PLUS_NATIVE_STEPWISE_TEST_RESULT',
    'CODEX_PLUS_NATIVE_UPDATE_METADATA_PATH',
    'CODEX_PLUS_NATIVE_UPDATE_ASSET_PATH',
    'CODEX_PLUS_NATIVE_UPDATE_LAUNCH_RECORD_PATH',
    'CODEX_PLUS_NATIVE_UPDATE_CHECK_RECORD_PATH',
    'CODEX_PLUS_NATIVE_ENV_PROCESS_ONLY',
    'OPENAI_CODEX_PLUS_PERF_SENTINEL'
)) {
    $PreviousEnvironment[$Name] = [Environment]::GetEnvironmentVariable($Name, 'Process')
}

try {
    New-Item -ItemType Directory -Path $RunDirectory | Out-Null
    Invoke-CargoBuild

    $ColdSamples = @()
    for ($Index = 1; $Index -le $ColdRunCount; $Index += 1) {
        $ColdSamples += Invoke-NativeSample `
            -Name "cold-$Index" `
            -ExitAfterMs $ColdExitAfterMs
    }

    $IdleSample = Invoke-NativeSample `
        -Name 'idle-30s' `
        -ExitAfterMs $IdleExitAfterMs `
        -MemorySampleAfterSeconds $IdleSampleSeconds

    $ColdFirstFrames = [double[]] @($ColdSamples | ForEach-Object { $_.FirstUiFrameMs })
    $CpuSamples = [double[]] @($IdleSample.CpuSamplesMs)
    $InputSamples = [double[]] @($IdleSample.InputSamplesMs)
    $AllStalls = [double[]] @($CpuSamples + $InputSamples)

    if ($CpuSamples.Count -eq 0) {
        throw 'the 30-second sample did not contain CPU frame samples'
    }
    if ($InputSamples.Count -ne $ExpectedScriptActions.Count) {
        throw "expected $($ExpectedScriptActions.Count) scripted input samples, got $($InputSamples.Count)"
    }
    if ($IdleSample.ScriptActions.Count -ne $ExpectedScriptActions.Count) {
        throw "expected $($ExpectedScriptActions.Count) scripted actions, got $($IdleSample.ScriptActions.Count)"
    }
    if (($IdleSample.ScriptActions -join "`n") -ne ($ExpectedScriptActions -join "`n")) {
        throw 'scripted action sequence did not match the named performance scenario'
    }
    Assert-ContextWorkflowResult -Sample $IdleSample
    Assert-MarketplaceWorkflowResult -Sample $IdleSample
    Assert-UserScriptWorkflowResult -Sample $IdleSample
    Assert-ZedRemoteWorkflowResult -Sample $IdleSample
    Assert-MaintenanceSettingsWorkflowResult -Sample $IdleSample
    if ($null -eq $IdleSample.PrivateMemoryBytes) {
        throw 'the 30-second sample did not record private memory'
    }

    $ColdMaximumMs = Get-Maximum -Values $ColdFirstFrames
    $CpuP95Ms = Get-Percentile -Values $CpuSamples -Percentile 0.95
    $MaximumStallMs = Get-Maximum -Values $AllStalls

    Write-Output "Evidence directory: $RunDirectory"
    Write-Output ("Cold first UI frames (ms): " + (($ColdFirstFrames | ForEach-Object {
        $_.ToString('F3', [Globalization.CultureInfo]::InvariantCulture)
    }) -join ', '))
    Write-Output ("CPU frame samples (ms): " + (($CpuSamples | ForEach-Object {
        $_.ToString('F3', [Globalization.CultureInfo]::InvariantCulture)
    }) -join ', '))
    Write-Output ("Input latency samples (ms): " + (($InputSamples | ForEach-Object {
        $_.ToString('F3', [Globalization.CultureInfo]::InvariantCulture)
    }) -join ', '))
    Write-Output ("Cold maximum: {0:F3} ms (limit {1:F1} ms)" -f $ColdMaximumMs, $FirstFrameLimitMs)
    Write-Output ("CPU p95: {0:F3} ms (limit {1:F1} ms)" -f $CpuP95Ms, $CpuP95LimitMs)
    Write-Output ("Maximum CPU/input stall: {0:F3} ms (limit < {1:F1} ms)" -f $MaximumStallMs, $MaximumStallLimitMs)
    Write-Output ("Private memory at 30s: {0} bytes (limit {1} bytes)" -f $IdleSample.PrivateMemoryBytes, $PrivateMemoryLimitBytes)

    $Failures = @()
    if ($ColdMaximumMs -gt $FirstFrameLimitMs) {
        $Failures += "cold maximum $ColdMaximumMs ms exceeds $FirstFrameLimitMs ms"
    }
    if ($CpuP95Ms -gt $CpuP95LimitMs) {
        $Failures += "CPU p95 $CpuP95Ms ms exceeds $CpuP95LimitMs ms"
    }
    if ($MaximumStallMs -ge $MaximumStallLimitMs) {
        $Failures += "maximum stall $MaximumStallMs ms is not below $MaximumStallLimitMs ms"
    }
    if ($IdleSample.PrivateMemoryBytes -gt $PrivateMemoryLimitBytes) {
        $Failures += "private memory $($IdleSample.PrivateMemoryBytes) exceeds $PrivateMemoryLimitBytes bytes"
    }

    if ($Failures.Count -gt 0) {
        $Failures | ForEach-Object { Write-Error $_ }
        exit 1
    }

    Write-Output 'Native manager performance gate passed.'
}
finally {
    foreach ($Name in $PreviousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($Name, $PreviousEnvironment[$Name], 'Process')
    }
}
