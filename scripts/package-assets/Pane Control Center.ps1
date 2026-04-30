param(
    [string]$SessionName = "pane",
    [switch]$PrintOnly
)

$ErrorActionPreference = "Stop"

$packageRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$paneExe = Join-Path $packageRoot "pane.exe"
$launchScript = Join-Path $packageRoot "Launch Pane Arch.ps1"
$shareScript = Join-Path $packageRoot "Open Pane Shared Folder.ps1"
$terminalScript = Join-Path $packageRoot "Open Pane Arch Terminal.ps1"
$bundleScript = Join-Path $packageRoot "Collect Pane Support Bundle.ps1"
$shortcutScript = Join-Path $packageRoot "Install Pane Shortcuts.ps1"

foreach ($path in @($paneExe, $launchScript, $shareScript, $terminalScript, $bundleScript, $shortcutScript)) {
    if (-not (Test-Path $path)) {
        throw "Expected package entrypoint missing: $path"
    }
}

if ($PrintOnly) {
    Write-Host "Pane Control Center"
    Write-Host "  Package Root      $packageRoot"
    Write-Host "  Session           $SessionName"
    Write-Host "  Supported Profile Arch Linux + XFCE"
    Write-Host "  Shared Storage   Durable by default; opt into scratch storage for disposable sessions."
    Write-Host "  Runtime Space    Dedicated Pane runtime storage can be prepared for the future contained OS engine."
    Write-Host "  Image Register   Local Arch base images can be copied into Pane runtime storage with SHA-256 metadata."
    Write-Host "  Loader Register  Controlled boot-to-serial loader candidates can be verified before WHP execution."
    Write-Host "  Kernel Register  Verified kernel/initramfs boot plans can be prepared before WHP kernel entry."
    Write-Host "  Kernel Layout    Materialized guest-memory layout for boot params, cmdline, kernel, and initramfs."
    Write-Host "  Native Preflight Probe Windows Hypervisor Platform readiness before the Pane-owned boot spike."
    Write-Host "  Boot Spike       Explicit WHP guest-memory, register, vCPU, and serial-I/O fixture for the next native-runtime milestone."
    Write-Host "  Native Preview   Pane-owned runtime dry-run does not invoke WSL, mstsc.exe, or XRDP."
    Write-Host "  First Run Wizard Onboard Arch, configure the Linux login, then launch the desktop from one app surface."
    Write-Host "  Display Transport Current mode is mstsc.exe + XRDP; embedded Pane window and native transport are roadmap modes."
    Write-Host "  Managed Flow      Onboard Arch to create or adopt the managed distro, configure the Arch login, and verify launch readiness."
    Write-Host "  Setup Bridge      Use Setup User later when only the Arch login or WSL config needs repair."
    Write-Host "  Terminal Bridge   Open Pane Arch Terminal for setup, package installs, and customization."
    Write-Host "  Current Boundary  Additional desktop profiles stay locked until the underlying support matrix is real."
    exit 0
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

function Invoke-PaneExe {
    param(
        [string[]]$Arguments
    )

    $output = & $paneExe @Arguments 2>&1
    $exitCode = $LASTEXITCODE
    $raw = ($output | Out-String).TrimEnd()
    return @{
        ExitCode = $exitCode
        Output = $raw
    }
}

function Invoke-PaneExeWithInput {
    param(
        [string[]]$Arguments,
        [string]$InputText
    )

    $output = $InputText | & $paneExe @Arguments 2>&1
    $exitCode = $LASTEXITCODE
    $raw = ($output | Out-String).TrimEnd()
    return @{
        ExitCode = $exitCode
        Output = $raw
    }
}

function Invoke-PackageScript {
    param(
        [string]$ScriptPath,
        [string[]]$Arguments
    )

    $output = & powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File $ScriptPath @Arguments 2>&1
    $exitCode = $LASTEXITCODE
    $raw = ($output | Out-String).TrimEnd()
    return @{
        ExitCode = $exitCode
        Output = $raw
    }
}

function Show-SetupUserDialog {
    param(
        [string]$SuggestedUser = "archuser",
        [bool]$DryRun = $false
    )

    $dialog = New-Object System.Windows.Forms.Form
    $dialog.Text = "Setup Arch User"
    $dialog.StartPosition = "CenterParent"
    $dialog.Size = New-Object System.Drawing.Size(520, 330)
    $dialog.MinimumSize = New-Object System.Drawing.Size(520, 330)
    $dialog.MaximizeBox = $false
    $dialog.MinimizeBox = $false
    $dialog.FormBorderStyle = "FixedDialog"
    $dialog.BackColor = [System.Drawing.Color]::FromArgb(247, 246, 243)
    $dialog.Font = New-Object System.Drawing.Font("Segoe UI", 10)

    $intro = New-Object System.Windows.Forms.Label
    $intro.Text = "Create or repair the Arch login, set it as the default WSL user, enable systemd, and optionally restart WSL so the change applies immediately."
    $intro.AutoSize = $false
    $intro.Size = New-Object System.Drawing.Size(470, 48)
    $intro.Location = New-Object System.Drawing.Point(18, 16)
    $dialog.Controls.Add($intro)

    $userLabel = New-Object System.Windows.Forms.Label
    $userLabel.Text = "Linux Username"
    $userLabel.AutoSize = $true
    $userLabel.Location = New-Object System.Drawing.Point(20, 78)
    $dialog.Controls.Add($userLabel)

    $userBox = New-Object System.Windows.Forms.TextBox
    $userBox.Text = $SuggestedUser
    $userBox.Location = New-Object System.Drawing.Point(20, 98)
    $userBox.Size = New-Object System.Drawing.Size(220, 28)
    $dialog.Controls.Add($userBox)

    $passwordLabel = New-Object System.Windows.Forms.Label
    $passwordLabel.Text = "Password"
    $passwordLabel.AutoSize = $true
    $passwordLabel.Location = New-Object System.Drawing.Point(20, 138)
    $dialog.Controls.Add($passwordLabel)

    $passwordBox = New-Object System.Windows.Forms.TextBox
    $passwordBox.Location = New-Object System.Drawing.Point(20, 158)
    $passwordBox.Size = New-Object System.Drawing.Size(220, 28)
    $passwordBox.PasswordChar = '*'
    $dialog.Controls.Add($passwordBox)

    $confirmLabel = New-Object System.Windows.Forms.Label
    $confirmLabel.Text = "Confirm Password"
    $confirmLabel.AutoSize = $true
    $confirmLabel.Location = New-Object System.Drawing.Point(260, 138)
    $dialog.Controls.Add($confirmLabel)

    $confirmBox = New-Object System.Windows.Forms.TextBox
    $confirmBox.Location = New-Object System.Drawing.Point(260, 158)
    $confirmBox.Size = New-Object System.Drawing.Size(220, 28)
    $confirmBox.PasswordChar = '*'
    $dialog.Controls.Add($confirmBox)

    $restartBox = New-Object System.Windows.Forms.CheckBox
    $restartBox.Text = "Restart WSL after setup (recommended)"
    $restartBox.AutoSize = $true
    $restartBox.Checked = $true
    $restartBox.Location = New-Object System.Drawing.Point(20, 205)
    $dialog.Controls.Add($restartBox)

    $hint = New-Object System.Windows.Forms.Label
    $hint.Text = if ($DryRun) { "Dry run is enabled, so the password fields are optional." } else { "Pane will pipe the password over stdin instead of putting it on the command line." }
    $hint.AutoSize = $false
    $hint.Size = New-Object System.Drawing.Size(460, 36)
    $hint.Location = New-Object System.Drawing.Point(20, 232)
    $dialog.Controls.Add($hint)

    $okButton = New-Object System.Windows.Forms.Button
    $okButton.Text = "Continue"
    $okButton.Location = New-Object System.Drawing.Point(294, 266)
    $okButton.Size = New-Object System.Drawing.Size(90, 30)
    $okButton.DialogResult = [System.Windows.Forms.DialogResult]::OK
    $dialog.Controls.Add($okButton)

    $cancelButton = New-Object System.Windows.Forms.Button
    $cancelButton.Text = "Cancel"
    $cancelButton.Location = New-Object System.Drawing.Point(390, 266)
    $cancelButton.Size = New-Object System.Drawing.Size(90, 30)
    $cancelButton.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
    $dialog.Controls.Add($cancelButton)

    $dialog.AcceptButton = $okButton
    $dialog.CancelButton = $cancelButton

    $result = $dialog.ShowDialog($form)
    if ($result -ne [System.Windows.Forms.DialogResult]::OK) {
        return $null
    }

    return @{
        Username = $userBox.Text.Trim()
        Password = $passwordBox.Text
        ConfirmPassword = $confirmBox.Text
        RestartWSL = $restartBox.Checked
    }
}

function Show-TextInputDialog {
    param(
        [string]$Title,
        [string]$Prompt
    )

    $dialog = New-Object System.Windows.Forms.Form
    $dialog.Text = $Title
    $dialog.Size = New-Object System.Drawing.Size(560, 210)
    $dialog.MinimumSize = New-Object System.Drawing.Size(560, 210)
    $dialog.StartPosition = "CenterParent"
    $dialog.FormBorderStyle = "FixedDialog"
    $dialog.MaximizeBox = $false
    $dialog.MinimizeBox = $false

    $label = New-Object System.Windows.Forms.Label
    $label.Text = $Prompt
    $label.AutoSize = $false
    $label.Size = New-Object System.Drawing.Size(500, 42)
    $label.Location = New-Object System.Drawing.Point(18, 18)
    $dialog.Controls.Add($label)

    $textBox = New-Object System.Windows.Forms.TextBox
    $textBox.Location = New-Object System.Drawing.Point(22, 72)
    $textBox.Size = New-Object System.Drawing.Size(500, 28)
    $dialog.Controls.Add($textBox)

    $okButton = New-Object System.Windows.Forms.Button
    $okButton.Text = "OK"
    $okButton.Location = New-Object System.Drawing.Point(326, 120)
    $okButton.Size = New-Object System.Drawing.Size(90, 30)
    $okButton.DialogResult = [System.Windows.Forms.DialogResult]::OK
    $dialog.Controls.Add($okButton)

    $cancelButton = New-Object System.Windows.Forms.Button
    $cancelButton.Text = "Cancel"
    $cancelButton.Location = New-Object System.Drawing.Point(432, 120)
    $cancelButton.Size = New-Object System.Drawing.Size(90, 30)
    $cancelButton.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
    $dialog.Controls.Add($cancelButton)

    $dialog.AcceptButton = $okButton
    $dialog.CancelButton = $cancelButton

    $result = $dialog.ShowDialog($form)
    if ($result -ne [System.Windows.Forms.DialogResult]::OK) {
        return $null
    }

    return $textBox.Text
}

$form = New-Object System.Windows.Forms.Form
$form.Text = "Pane Control Center"
$form.StartPosition = "CenterScreen"
$form.Size = New-Object System.Drawing.Size(860, 720)
$form.MinimumSize = New-Object System.Drawing.Size(860, 720)
$form.BackColor = [System.Drawing.Color]::FromArgb(247, 246, 243)
$form.Font = New-Object System.Drawing.Font("Segoe UI", 10)

$title = New-Object System.Windows.Forms.Label
$title.Text = "Pane Control Center"
$title.Font = New-Object System.Drawing.Font("Segoe UI", 20, [System.Drawing.FontStyle]::Bold)
$title.AutoSize = $true
$title.Location = New-Object System.Drawing.Point(24, 18)
$form.Controls.Add($title)

$subtitle = New-Object System.Windows.Forms.Label
$subtitle.Text = "Initialize a Pane-managed Arch environment first, then launch Arch + XFCE from here. Durable PaneShared storage is preserved across session reset unless you choose scratch storage."
$subtitle.AutoSize = $false
$subtitle.Size = New-Object System.Drawing.Size(790, 44)
$subtitle.Location = New-Object System.Drawing.Point(26, 58)
$form.Controls.Add($subtitle)

$sessionLabel = New-Object System.Windows.Forms.Label
$sessionLabel.Text = "Session"
$sessionLabel.AutoSize = $true
$sessionLabel.Location = New-Object System.Drawing.Point(28, 112)
$form.Controls.Add($sessionLabel)

$sessionBox = New-Object System.Windows.Forms.TextBox
$sessionBox.Text = $SessionName
$sessionBox.Location = New-Object System.Drawing.Point(28, 132)
$sessionBox.Size = New-Object System.Drawing.Size(220, 28)
$form.Controls.Add($sessionBox)

$profileLabel = New-Object System.Windows.Forms.Label
$profileLabel.Text = "Desktop Profile"
$profileLabel.AutoSize = $true
$profileLabel.Location = New-Object System.Drawing.Point(270, 112)
$form.Controls.Add($profileLabel)

$profileValue = New-Object System.Windows.Forms.TextBox
$profileValue.Text = "Arch Linux + XFCE (locked for MVP)"
$profileValue.ReadOnly = $true
$profileValue.Location = New-Object System.Drawing.Point(270, 132)
$profileValue.Size = New-Object System.Drawing.Size(280, 28)
$form.Controls.Add($profileValue)

$transportLabel = New-Object System.Windows.Forms.Label
$transportLabel.Text = "Transport"
$transportLabel.AutoSize = $true
$transportLabel.Location = New-Object System.Drawing.Point(572, 112)
$form.Controls.Add($transportLabel)

$transportValue = New-Object System.Windows.Forms.TextBox
$transportValue.Text = "mstsc.exe + XRDP (direct localhost or Pane relay)"
$transportValue.ReadOnly = $true
$transportValue.Location = New-Object System.Drawing.Point(572, 132)
$transportValue.Size = New-Object System.Drawing.Size(250, 28)
$form.Controls.Add($transportValue)

$dryRunBox = New-Object System.Windows.Forms.CheckBox
$dryRunBox.Text = "Dry run actions"
$dryRunBox.AutoSize = $true
$dryRunBox.Location = New-Object System.Drawing.Point(28, 172)
$form.Controls.Add($dryRunBox)

$noConnectBox = New-Object System.Windows.Forms.CheckBox
$noConnectBox.Text = "Do not open mstsc.exe"
$noConnectBox.AutoSize = $true
$noConnectBox.Location = New-Object System.Drawing.Point(150, 172)
$form.Controls.Add($noConnectBox)

$scratchSharedBox = New-Object System.Windows.Forms.CheckBox
$scratchSharedBox.Text = "Scratch PaneShared"
$scratchSharedBox.AutoSize = $true
$scratchSharedBox.Location = New-Object System.Drawing.Point(328, 172)
$form.Controls.Add($scratchSharedBox)

$statusBox = New-Object System.Windows.Forms.TextBox
$statusBox.Multiline = $true
$statusBox.ReadOnly = $true
$statusBox.ScrollBars = "Vertical"
$statusBox.Location = New-Object System.Drawing.Point(28, 420)
$statusBox.Size = New-Object System.Drawing.Size(794, 224)
$statusBox.BackColor = [System.Drawing.Color]::White
$statusBox.Font = New-Object System.Drawing.Font("Consolas", 10)
$form.Controls.Add($statusBox)

function Set-StatusText {
    param(
        [string]$Text
    )

    $statusBox.Text = $Text
    $statusBox.SelectionStart = $statusBox.TextLength
    $statusBox.ScrollToCaret()
}

function Run-Action {
    param(
        [string]$Title,
        [scriptblock]$Body
    )

    $form.UseWaitCursor = $true
    Set-StatusText ("[{0}] Starting...`r`nPane is running the requested step. If this touches WSL, it can take a few minutes on first run." -f $Title)
    [System.Windows.Forms.Application]::DoEvents()
    try {
        $result = & $Body
        $output = if ([string]::IsNullOrWhiteSpace($result.Output)) { "(no output)" } else { $result.Output }
        Set-StatusText ("[{0}] Exit {1}`r`n`r`n{2}" -f $Title, $result.ExitCode, $output)
        if ($result.ExitCode -ne 0) {
            [System.Windows.Forms.MessageBox]::Show($form, $output, "Pane", [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
        }
        return $result
    }
    finally {
        $form.UseWaitCursor = $false
        [System.Windows.Forms.Application]::DoEvents()
    }
}

function Refresh-Overview {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) {
        $session = "pane"
        $sessionBox.Text = $session
    }

    $appStatus = Invoke-PaneExe -Arguments @("app-status", "--json", "--session-name", $session)

    $lines = @()
    $lines += "Pane Control Center"
    $lines += "Session         $session"
    $lines += "PaneShared      $(if ($scratchSharedBox.Checked) { "scratch session storage" } else { "durable user storage" })"
    $lines += ""

    if ($appStatus.ExitCode -eq 0) {
        try {
            $appJson = $appStatus.Output | ConvertFrom-Json
            $lines += "App Phase       $($appJson.phase)"
            $lines += "Next Step       $($appJson.next_action_label)"
            $lines += "Why             $($appJson.next_action_summary)"
            $lines += "Profile         $($appJson.supported_profile.label)"
            $lines += "Runtime Current $($appJson.runtime.current_engine_label)"
            $lines += "Runtime Target  $($appJson.runtime.target_engine_label)"
            $lines += "Runtime Ready   $($appJson.runtime.prepared)"
            if ($appJson.runtime.native_runtime) {
                $lines += "Native Host     $($appJson.runtime.native_runtime.host_ready)"
                $lines += "Boot Spike      $($appJson.runtime.native_runtime.ready_for_boot_spike)"
            }
            if ($appJson.runtime.artifacts) {
                $lines += "Kernel Layout   $($appJson.runtime.artifacts.kernel_boot_layout_ready)"
            }
            if ($appJson.runtime.native_host -and $appJson.runtime.native_host.whp) {
                $lines += "WHP Library     $($appJson.runtime.native_host.whp.dll_loaded)"
                $lines += "WHP Hypervisor  $($appJson.runtime.native_host.whp.hypervisor_present)"
            }
            $lines += "Runtime Root    $($appJson.runtime.dedicated_space_root)"
            $lines += "Display         $($appJson.display.current_mode_label)"
            $lines += "Contained App   $($appJson.display.contained_window_available)"
            $lines += "Visible Handoff $($appJson.display.user_visible_handoff)"
            $lines += ""
            if ($appJson.managed_environment) {
                $lines += "Managed Distro  $($appJson.managed_environment.distro_name)"
                $lines += "Ownership       $($appJson.managed_environment.ownership)"
            }
            else {
                $lines += "Managed Distro  not initialized"
            }
            if ($appJson.selected_distro) {
                $lines += "Selected Distro $($appJson.selected_distro.distro.name)"
                $lines += "Default User    $($appJson.selected_distro.distro.default_user)"
                $lines += "Password        $($appJson.selected_distro.default_user_password_status)"
            }
            if ($appJson.last_launch) {
                $lines += "Last Launch     $($appJson.last_launch.stage)"
                if ($appJson.last_launch.transport) {
                    $lines += "Last Transport  $($appJson.last_launch.transport)"
                }
            }
            $lines += ""
            if ($appJson.blockers.Count -gt 0) {
                $lines += "Blockers"
                foreach ($blocker in $appJson.blockers) {
                    $lines += "  [$($blocker.id)] $($blocker.summary)"
                }
                $lines += ""
            }
            $lines += "Storage Policy  $($appJson.storage.policy)"
            $lines += "Durable Shared  $($appJson.storage.durable_shared_dir)"
            $lines += "Scratch Shared  $($appJson.storage.scratch_shared_dir)"
            $lines += "Runtime Budget  $($appJson.runtime.storage_budget.requested_capacity_gib) GiB total; $($appJson.runtime.storage_budget.user_packages_and_customizations_gib) GiB for packages/customizations"
            $lines += ""
            foreach ($note in $appJson.notes) {
                $lines += "Note            $note"
            }
        }
        catch {
            $lines += "App Status      Unable to parse app-status JSON"
            $lines += $appStatus.Output
        }
    }
    else {
        $lines += "App Status      Failed"
        $lines += $appStatus.Output
    }

    $lines += ""
    $lines += "First Run       Use Start First Run or Onboard Arch for managed distro, login user, systemd, and readiness verification."
    $lines += "Launch          Launch Arch prepares the session, picks the best current RDP transport, and opens the desktop handoff."
    $lines += "Repair          Repair Arch re-applies Pane-owned session assets when reconnect or a blank desktop fails."
    $lines += "Other desktop profiles stay hidden here until their launch, bootstrap, and recovery path is supportable."

    Set-StatusText ($lines -join "`r`n")
}

function Invoke-OnboardArchFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    $suggestedUser = "archuser"
    $managedDistro = $null
    $status = Invoke-PaneExe -Arguments @("status", "--json")
    if ($status.ExitCode -eq 0) {
        try {
            $statusJson = $status.Output | ConvertFrom-Json
            if ($statusJson.managed_environment -and $statusJson.managed_environment.distro_name) {
                $managedDistro = [string]$statusJson.managed_environment.distro_name
            }
            if ($statusJson.selected_distro -and $statusJson.selected_distro.distro.default_user -and $statusJson.selected_distro.distro.default_user -ne "root") {
                $suggestedUser = [string]$statusJson.selected_distro.distro.default_user
            }
        }
        catch {
        }
    }

    $dialog = Show-SetupUserDialog -SuggestedUser $suggestedUser -DryRun $dryRunBox.Checked
    if ($null -eq $dialog) {
        return @{ ExitCode = 0; Output = "Onboarding canceled." }
    }
    if ([string]::IsNullOrWhiteSpace($dialog.Username)) {
        return @{ ExitCode = 1; Output = "Enter a Linux username before continuing." }
    }

    $arguments = @("onboard", "--username", $dialog.Username, "--session-name", $session, "--de", "xfce")
    if ($managedDistro) {
        $arguments += @("--existing-distro", $managedDistro)
    }
    if (-not $dialog.RestartWSL) { $arguments += "--no-shutdown" }
    if ($dryRunBox.Checked) {
        $arguments += "--dry-run"
        return (Invoke-PaneExe -Arguments $arguments)
    }
    if ([string]::IsNullOrWhiteSpace($dialog.Password)) {
        return @{ ExitCode = 1; Output = "Enter a password before continuing." }
    }
    if ($dialog.Password -ne $dialog.ConfirmPassword) {
        return @{ ExitCode = 1; Output = "The password confirmation does not match." }
    }

    $arguments += "--password-stdin"
    return (Invoke-PaneExeWithInput -Arguments $arguments -InputText $dialog.Password)
}

function Invoke-LaunchArchFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
    $arguments = @("-SessionName", $session)
    $arguments += @("-SharedStorage", $(if ($scratchSharedBox.Checked) { "scratch" } else { "durable" }))
    if ($dryRunBox.Checked) { $arguments += "-DryRun" }
    if ($noConnectBox.Checked) { $arguments += "-NoConnect" }
    Invoke-PackageScript -ScriptPath $launchScript -Arguments $arguments
}

function Invoke-GuidedFirstRunFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    $appStatus = Invoke-PaneExe -Arguments @("app-status", "--json", "--session-name", $session)
    if ($appStatus.ExitCode -ne 0) {
        return $appStatus
    }

    try {
        $appJson = $appStatus.Output | ConvertFrom-Json
    }
    catch {
        return @{ ExitCode = 1; Output = "Pane could not parse app-status JSON.`r`n$($appStatus.Output)" }
    }

    $summary = "Pane phase: $($appJson.phase)`r`nNext step: $($appJson.next_action_label)`r`n`r`n$($appJson.next_action_summary)"
    $choice = [System.Windows.Forms.MessageBox]::Show(
        $form,
        $summary,
        "Pane First Run",
        [System.Windows.Forms.MessageBoxButtons]::OKCancel,
        [System.Windows.Forms.MessageBoxIcon]::Information
    )
    if ($choice -ne [System.Windows.Forms.DialogResult]::OK) {
        return @{ ExitCode = 0; Output = "First-run flow canceled." }
    }

    switch ([string]$appJson.next_action) {
        "install-wsl" {
            return @{ ExitCode = 1; Output = "Install WSL2 from Windows first, then reopen Pane and run Start First Run again." }
        }
        { $_ -in @("onboard-arch", "setup-user") } {
            return (Invoke-OnboardArchFlow)
        }
        "launch-arch" {
            return (Invoke-LaunchArchFlow)
        }
        "reconnect" {
            return (Invoke-PaneExe -Arguments @("connect", "--session-name", $session))
        }
        "repair-arch" {
            $arguments = @("repair", "--de", "xfce", "--session-name", $session, "--shared-storage", $(if ($scratchSharedBox.Checked) { "scratch" } else { "durable" }))
            if ($dryRunBox.Checked) { $arguments += "--dry-run" }
            return (Invoke-PaneExe -Arguments $arguments)
        }
        default {
            return @{ ExitCode = 0; Output = "Pane recommends: $($appJson.next_action_label). Use the matching Control Center button for details." }
        }
    }
}

function Invoke-PrepareRuntimeFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    $choice = [System.Windows.Forms.MessageBox]::Show(
        $form,
        "Pane will prepare dedicated app storage for the future contained OS runtime: downloads, OS images, a user disk descriptor, snapshots, and runtime state. This does not replace the current WSL/XRDP bridge yet.`r`n`r`nCreate the 8 GiB runtime reservation layout now?",
        "Prepare Pane Runtime Space",
        [System.Windows.Forms.MessageBoxButtons]::OKCancel,
        [System.Windows.Forms.MessageBoxIcon]::Information
    )
    if ($choice -ne [System.Windows.Forms.DialogResult]::OK) {
        return @{ ExitCode = 0; Output = "Runtime preparation canceled." }
    }

    return (Invoke-PaneExe -Arguments @("runtime", "--prepare", "--create-user-disk", "--create-serial-boot-image", "--capacity-gib", "8", "--session-name", $session))
}

function Invoke-NativePreflightFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    return (Invoke-PaneExe -Arguments @("native-preflight", "--session-name", $session))
}

function Invoke-NativeBootSpikeFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    $choice = [System.Windows.Forms.MessageBox]::Show(
        $form,
        "Pane will create a temporary Windows Hypervisor Platform partition and one virtual processor, map a deterministic serial test image, configure registers, run until the PANE_BOOT_OK COM1 banner and final HLT are observed, then tear everything down. This does not boot Arch yet and does not persist a VM.`r`n`r`nRun the WHP serial test image now?",
        "Pane Native Boot Spike",
        [System.Windows.Forms.MessageBoxButtons]::OKCancel,
        [System.Windows.Forms.MessageBoxIcon]::Information
    )
    if ($choice -ne [System.Windows.Forms.DialogResult]::OK) {
        return @{ ExitCode = 0; Output = "Native boot spike canceled." }
    }

    return (Invoke-PaneExe -Arguments @("native-boot-spike", "--execute", "--run-fixture", "--session-name", $session))
}

function Invoke-RegisterBaseImageFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = "Select Pane Arch Base OS Image"
    $dialog.Filter = "Pane/Linux images (*.paneimg;*.img;*.raw;*.qcow2;*.vhdx)|*.paneimg;*.img;*.raw;*.qcow2;*.vhdx|All files (*.*)|*.*"
    $dialog.CheckFileExists = $true
    $dialog.Multiselect = $false
    if ($dialog.ShowDialog($form) -ne [System.Windows.Forms.DialogResult]::OK) {
        return @{ ExitCode = 0; Output = "Base image registration canceled." }
    }

    $sha = Show-TextInputDialog -Title "Expected SHA-256" -Prompt "Paste the expected 64-character SHA-256 digest for this base image. Leave blank to register it as untrusted metadata only."
    if ($null -eq $sha) {
        return @{ ExitCode = 0; Output = "Base image registration canceled." }
    }

    $arguments = @("runtime", "--prepare", "--register-base-image", $dialog.FileName, "--session-name", $session)
    if (-not [string]::IsNullOrWhiteSpace($sha)) {
        $arguments += "--expected-sha256"
        $arguments += $sha.Trim()
    }

    return (Invoke-PaneExe -Arguments $arguments)
}

function Invoke-RegisterBootLoaderFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = "Select Pane Boot-to-Serial Loader Image"
    $dialog.Filter = "Pane boot images (*.paneimg;*.img;*.bin;*.raw)|*.paneimg;*.img;*.bin;*.raw|All files (*.*)|*.*"
    $dialog.CheckFileExists = $true
    $dialog.Multiselect = $false
    if ($dialog.ShowDialog($form) -ne [System.Windows.Forms.DialogResult]::OK) {
        return @{ ExitCode = 0; Output = "Boot-loader registration canceled." }
    }

    $sha = Show-TextInputDialog -Title "Expected SHA-256" -Prompt "Paste the expected 64-character SHA-256 digest. Pane will not treat the loader as executable unless this matches."
    if ($null -eq $sha) {
        return @{ ExitCode = 0; Output = "Boot-loader registration canceled." }
    }
    if ([string]::IsNullOrWhiteSpace($sha)) {
        return @{ ExitCode = 1; Output = "Boot-loader registration requires an expected SHA-256 digest." }
    }

    $serial = Show-TextInputDialog -Title "Expected Serial Text" -Prompt "Enter the exact serial text this loader must emit before HLT. Use \n for newline."
    if ($null -eq $serial) {
        return @{ ExitCode = 0; Output = "Boot-loader registration canceled." }
    }
    if ([string]::IsNullOrWhiteSpace($serial)) {
        $serial = "PANE_BOOT_OK\n"
    }

    return (Invoke-PaneExe -Arguments @("runtime", "--prepare", "--register-boot-loader", $dialog.FileName, "--boot-loader-expected-sha256", $sha.Trim(), "--boot-loader-expected-serial", $serial, "--session-name", $session))
}

function Invoke-RegisterKernelFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = "Select Pane Kernel Image"
    $dialog.Filter = "Kernel images (vmlinuz*;*.img;*.bin;*.raw;*.paneimg)|vmlinuz*;*.img;*.bin;*.raw;*.paneimg|All files (*.*)|*.*"
    $dialog.CheckFileExists = $true
    $dialog.Multiselect = $false
    if ($dialog.ShowDialog($form) -ne [System.Windows.Forms.DialogResult]::OK) {
        return @{ ExitCode = 0; Output = "Kernel boot-plan registration canceled." }
    }

    $sha = Show-TextInputDialog -Title "Expected Kernel SHA-256" -Prompt "Paste the expected 64-character SHA-256 digest. Pane will not mark the kernel boot plan ready unless this matches."
    if ($null -eq $sha) {
        return @{ ExitCode = 0; Output = "Kernel boot-plan registration canceled." }
    }
    if ([string]::IsNullOrWhiteSpace($sha)) {
        return @{ ExitCode = 1; Output = "Kernel boot-plan registration requires an expected SHA-256 digest." }
    }

    $cmdline = Show-TextInputDialog -Title "Kernel Command Line" -Prompt "Enter the kernel cmdline. It must include console=ttyS0 so Pane can observe boot progress."
    if ($null -eq $cmdline) {
        return @{ ExitCode = 0; Output = "Kernel boot-plan registration canceled." }
    }
    if ([string]::IsNullOrWhiteSpace($cmdline)) {
        $cmdline = "console=ttyS0 panic=-1"
    }

    return (Invoke-PaneExe -Arguments @("runtime", "--prepare", "--register-kernel", $dialog.FileName, "--kernel-expected-sha256", $sha.Trim(), "--kernel-cmdline", $cmdline, "--session-name", $session))
}

function Invoke-NativeKernelPlanFlow {
    $session = $sessionBox.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }

    return (Invoke-PaneExe -Arguments @("native-kernel-plan", "--materialize", "--session-name", $session))
}

$buttonSpecs = @(
    @{ Text = "Refresh"; Left = 28; Top = 214; Width = 126; Action = { Refresh-Overview; @{ ExitCode = 0; Output = $statusBox.Text } } },
    @{ Text = "Onboard Arch"; Left = 166; Top = 214; Width = 126; Action = { Invoke-OnboardArchFlow } },
    @{ Text = "Launch Arch"; Left = 304; Top = 214; Width = 126; Action = { Invoke-LaunchArchFlow } },
    @{ Text = "Doctor"; Left = 442; Top = 214; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            Invoke-PaneExe -Arguments @("doctor", "--de", "xfce", "--session-name", $session)
        } },
    @{ Text = "Reconnect"; Left = 580; Top = 214; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            Invoke-PaneExe -Arguments @("connect", "--session-name", $session)
        } },
    @{ Text = "Shared Folder"; Left = 718; Top = 214; Width = 104; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            Invoke-PackageScript -ScriptPath $shareScript -Arguments @("-SessionName", $session, "-SharedStorage", $(if ($scratchSharedBox.Checked) { "scratch" } else { "durable" }))
        } },
    @{ Text = "Logs"; Left = 28; Top = 260; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            Invoke-PaneExe -Arguments @("logs", "--session-name", $session)
        } },
    @{ Text = "Support Bundle"; Left = 166; Top = 260; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            Invoke-PackageScript -ScriptPath $bundleScript -Arguments @("-SessionName", $session)
        } },
    @{ Text = "Reset Session"; Left = 304; Top = 260; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            $choice = [System.Windows.Forms.MessageBox]::Show(
                $form,
                "Yes = reset the current session assets. Durable PaneShared is preserved unless you use pane reset --purge-shared.`r`nNo = release or factory reset the managed Arch environment.`r`nCancel = do nothing.",
                "Pane",
                [System.Windows.Forms.MessageBoxButtons]::YesNoCancel,
                [System.Windows.Forms.MessageBoxIcon]::Question
            )
            if ($choice -eq [System.Windows.Forms.DialogResult]::Cancel) {
                return @{ ExitCode = 0; Output = "Reset canceled." }
            }
            if ($choice -eq [System.Windows.Forms.DialogResult]::Yes) {
                return (Invoke-PaneExe -Arguments @("reset", "--session-name", $session))
            }

            $status = Invoke-PaneExe -Arguments @("status", "--json")
            if ($status.ExitCode -ne 0) {
                return $status
            }

            try {
                $statusJson = $status.Output | ConvertFrom-Json
                if (-not $statusJson.managed_environment) {
                    return @{ ExitCode = 0; Output = "Pane is not currently managing a distro." }
                }

                if ($statusJson.managed_environment.ownership -in @("imported-rootfs", "installed-online")) {
                    $confirm = [System.Windows.Forms.MessageBox]::Show(
                        $form,
                        "Factory reset will unregister the Pane-provisioned distro $($statusJson.managed_environment.distro_name), remove its install root, and clear Pane ownership. Continue?",
                        "Pane",
                        [System.Windows.Forms.MessageBoxButtons]::YesNo,
                        [System.Windows.Forms.MessageBoxIcon]::Warning
                    )
                    if ($confirm -ne [System.Windows.Forms.DialogResult]::Yes) {
                        return @{ ExitCode = 0; Output = "Factory reset canceled." }
                    }
                    return (Invoke-PaneExe -Arguments @("reset", "--session-name", $session, "--factory-reset"))
                }

                $confirm = [System.Windows.Forms.MessageBox]::Show(
                    $form,
                    "Release Pane management for $($statusJson.managed_environment.distro_name) without deleting the distro? Pane will also purge its WSL session wiring.",
                    "Pane",
                    [System.Windows.Forms.MessageBoxButtons]::YesNo,
                    [System.Windows.Forms.MessageBoxIcon]::Question
                )
                if ($confirm -ne [System.Windows.Forms.DialogResult]::Yes) {
                    return @{ ExitCode = 0; Output = "Managed-environment release canceled." }
                }
                return (Invoke-PaneExe -Arguments @("reset", "--session-name", $session, "--release-managed-environment", "--purge-wsl"))
            }
            catch {
                return @{ ExitCode = 1; Output = "Pane could not parse status JSON for the reset flow." }
            }
        } },
    @{ Text = "Repair Arch"; Left = 442; Top = 260; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            $arguments = @("repair", "--de", "xfce", "--session-name", $session, "--shared-storage", $(if ($scratchSharedBox.Checked) { "scratch" } else { "durable" }))
            if ($dryRunBox.Checked) { $arguments += "--dry-run" }
            Invoke-PaneExe -Arguments $arguments
        } },
    @{ Text = "Update Arch"; Left = 580; Top = 260; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            $arguments = @("update", "--de", "xfce", "--session-name", $session, "--shared-storage", $(if ($scratchSharedBox.Checked) { "scratch" } else { "durable" }))
            if ($dryRunBox.Checked) { $arguments += "--dry-run" }
            Invoke-PaneExe -Arguments $arguments
        } },
    @{ Text = "Install Shortcuts"; Left = 28; Top = 306; Width = 126; Action = {
            Invoke-PackageScript -ScriptPath $shortcutScript -Arguments @()
        } },
    @{ Text = "Arch Terminal"; Left = 166; Top = 306; Width = 126; Action = {
            Start-Process powershell.exe -ArgumentList @(
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                $terminalScript
            ) | Out-Null
            @{ ExitCode = 0; Output = "Opened a Pane Arch terminal window." }
        } },
    @{ Text = "Setup User"; Left = 304; Top = 306; Width = 126; Action = {
            $suggestedUser = "archuser"
            $status = Invoke-PaneExe -Arguments @("status", "--json")
            if ($status.ExitCode -eq 0) {
                try {
                    $statusJson = $status.Output | ConvertFrom-Json
                    if ($statusJson.selected_distro -and $statusJson.selected_distro.distro.default_user -and $statusJson.selected_distro.distro.default_user -ne "root") {
                        $suggestedUser = $statusJson.selected_distro.distro.default_user
                    }
                }
                catch {
                }
            }

            $dialog = Show-SetupUserDialog -SuggestedUser $suggestedUser -DryRun $dryRunBox.Checked
            if ($null -eq $dialog) {
                return @{ ExitCode = 0; Output = "User setup canceled." }
            }
            if ([string]::IsNullOrWhiteSpace($dialog.Username)) {
                return @{ ExitCode = 1; Output = "Enter a Linux username before continuing." }
            }

            $arguments = @("setup-user", "--username", $dialog.Username)
            if (-not $dialog.RestartWSL) { $arguments += "--no-shutdown" }
            if ($dryRunBox.Checked) {
                $arguments += "--dry-run"
                return (Invoke-PaneExe -Arguments $arguments)
            }
            if ([string]::IsNullOrWhiteSpace($dialog.Password)) {
                return @{ ExitCode = 1; Output = "Enter a password before continuing." }
            }
            if ($dialog.Password -ne $dialog.ConfirmPassword) {
                return @{ ExitCode = 1; Output = "The password confirmation does not match." }
            }

            $arguments += "--password-stdin"
            return (Invoke-PaneExeWithInput -Arguments $arguments -InputText $dialog.Password)
        } },
    @{ Text = "Start First Run"; Left = 442; Top = 306; Width = 126; Action = {
            Invoke-GuidedFirstRunFlow
        } },
    @{ Text = "Prepare Runtime"; Left = 580; Top = 306; Width = 126; Action = {
            Invoke-PrepareRuntimeFlow
        } },
    @{ Text = "Boot Spike"; Left = 304; Top = 352; Width = 126; Action = {
            Invoke-NativeBootSpikeFlow
        } },
    @{ Text = "Native Preflight"; Left = 442; Top = 352; Width = 126; Action = {
            Invoke-NativePreflightFlow
        } },
    @{ Text = "Register Image"; Left = 580; Top = 352; Width = 126; Action = {
            Invoke-RegisterBaseImageFlow
        } },
    @{ Text = "Register Loader"; Left = 28; Top = 352; Width = 126; Action = {
            Invoke-RegisterBootLoaderFlow
        } },
    @{ Text = "Register Kernel"; Left = 166; Top = 352; Width = 126; Action = {
            Invoke-RegisterKernelFlow
        } },
    @{ Text = "Kernel Layout"; Left = 718; Top = 306; Width = 104; Action = {
            Invoke-NativeKernelPlanFlow
        } },
    @{ Text = "Native Runtime"; Left = 718; Top = 260; Width = 104; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            Invoke-PaneExe -Arguments @("launch", "--runtime", "pane-owned", "--dry-run", "--session-name", $session)
        } },
    @{ Text = "Close"; Left = 718; Top = 352; Width = 104; Action = {
            $form.Close()
            @{ ExitCode = 0; Output = "Pane Control Center closed." }
        } }
)

foreach ($spec in $buttonSpecs) {
    $button = New-Object System.Windows.Forms.Button
    $button.Text = $spec.Text
    $button.Location = New-Object System.Drawing.Point($spec.Left, $spec.Top)
    $button.Size = New-Object System.Drawing.Size($spec.Width, 34)
    $action = $spec.Action
    $handler = {
        Run-Action -Title $this.Text -Body $action | Out-Null
    }.GetNewClosure()
    $button.Add_Click($handler)
    $form.Controls.Add($button)
}

$form.Add_Shown({ Refresh-Overview })
[void]$form.ShowDialog()



