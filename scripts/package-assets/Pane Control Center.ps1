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
    Write-Host "  Managed Flow      Onboard Arch to create or adopt the managed distro, configure the Arch login, and verify launch readiness."
    Write-Host "  Setup Bridge      Use Setup User later when only the Arch login or WSL config needs repair."
    Write-Host "  Terminal Bridge   Open Pane Arch Terminal for setup, package installs, and customization."
    Write-Host "  Planned Profiles  KDE Plasma, GNOME, Niri"
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

$form = New-Object System.Windows.Forms.Form
$form.Text = "Pane Control Center"
$form.StartPosition = "CenterScreen"
$form.Size = New-Object System.Drawing.Size(860, 660)
$form.MinimumSize = New-Object System.Drawing.Size(860, 660)
$form.BackColor = [System.Drawing.Color]::FromArgb(247, 246, 243)
$form.Font = New-Object System.Drawing.Font("Segoe UI", 10)

$title = New-Object System.Windows.Forms.Label
$title.Text = "Pane Control Center"
$title.Font = New-Object System.Drawing.Font("Segoe UI", 20, [System.Drawing.FontStyle]::Bold)
$title.AutoSize = $true
$title.Location = New-Object System.Drawing.Point(24, 18)
$form.Controls.Add($title)

$subtitle = New-Object System.Windows.Forms.Label
$subtitle.Text = "Initialize a Pane-managed Arch environment first, then launch Arch + XFCE from here. KDE, GNOME, and Niri stay locked until their support path is real."
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

$statusBox = New-Object System.Windows.Forms.TextBox
$statusBox.Multiline = $true
$statusBox.ReadOnly = $true
$statusBox.ScrollBars = "Vertical"
$statusBox.Location = New-Object System.Drawing.Point(28, 374)
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

    $status = Invoke-PaneExe -Arguments @("status", "--json")
    $doctor = Invoke-PaneExe -Arguments @("doctor", "--json", "--de", "xfce", "--session-name", $session)

    $lines = @()
    $lines += "Pane Control Center"
    $lines += "Session         $session"
    $lines += "Profile         Arch Linux + XFCE"
    $lines += "Transport       mstsc.exe + XRDP"
    $lines += ""

    if ($status.ExitCode -eq 0) {
        try {
            $statusJson = $status.Output | ConvertFrom-Json
            $lines += "WSL Available   $($statusJson.wsl_available)"
            if ($statusJson.managed_environment) {
                $lines += "Managed Distro  $($statusJson.managed_environment.distro_name)"
                $lines += "Ownership       $($statusJson.managed_environment.ownership)"
            }
            else {
                $lines += "Managed Distro  not initialized"
            }
            if ($statusJson.selected_distro) {
                $lines += "Selected Distro $($statusJson.selected_distro.distro.name)"
                $lines += "Default User    $($statusJson.selected_distro.distro.default_user)"
            }
        }
        catch {
            $lines += "Status          Unable to parse status JSON"
        }
    }
    else {
        $lines += "Status          Failed"
        $lines += $status.Output
    }

    $lines += ""

    if ($doctor.ExitCode -eq 0) {
        try {
            $doctorJson = $doctor.Output | ConvertFrom-Json
            $lines += "Doctor Ready    $($doctorJson.ready)"
            $lines += "Supported MVP   $($doctorJson.supported_for_mvp)"
            $lines += "Target Distro   $($doctorJson.target_distro)"
        }
        catch {
            $lines += "Doctor          Unable to parse doctor JSON"
        }
    }
    else {
        $lines += "Doctor          Failed"
        $lines += $doctor.Output
    }

    $lines += ""
    $lines += "Onboard Arch    Use Onboard Arch for first-run setup: managed distro, login user, systemd, and readiness verification."
    $lines += "Setup User      Use Setup User when only the Arch login or WSL config needs repair."
    $lines += "Arch Terminal   Use the terminal bridge for passwd, pacman, dotfiles, and shell-level customization."
    $lines += "Other desktop profiles stay hidden here until their launch, bootstrap, and recovery path is supportable."

    Set-StatusText ($lines -join "`r`n")
}

$buttonSpecs = @(
    @{ Text = "Refresh"; Left = 28; Top = 214; Width = 126; Action = { Refresh-Overview; @{ ExitCode = 0; Output = $statusBox.Text } } },
    @{ Text = "Onboard Arch"; Left = 166; Top = 214; Width = 126; Action = {
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
        } },
    @{ Text = "Launch Arch"; Left = 304; Top = 214; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            $arguments = @("-SessionName", $session)
            if ($dryRunBox.Checked) { $arguments += "-DryRun" }
            if ($noConnectBox.Checked) { $arguments += "-NoConnect" }
            Invoke-PackageScript -ScriptPath $launchScript -Arguments $arguments
        } },
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
            Invoke-PackageScript -ScriptPath $shareScript -Arguments @("-SessionName", $session)
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
                "Yes = reset the current session assets.`r`nNo = release or factory reset the managed Arch environment.`r`nCancel = do nothing.",
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
            $arguments = @("repair", "--de", "xfce", "--session-name", $session)
            if ($dryRunBox.Checked) { $arguments += "--dry-run" }
            Invoke-PaneExe -Arguments $arguments
        } },
    @{ Text = "Update Arch"; Left = 580; Top = 260; Width = 126; Action = {
            $session = $sessionBox.Text.Trim()
            if ([string]::IsNullOrWhiteSpace($session)) { $session = "pane"; $sessionBox.Text = $session }
            $arguments = @("update", "--de", "xfce", "--session-name", $session)
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
    @{ Text = "Open Package Folder"; Left = 718; Top = 260; Width = 104; Action = {
            Start-Process explorer.exe $packageRoot | Out-Null
            @{ ExitCode = 0; Output = "Opened $packageRoot" }
        } },
    @{ Text = "Close"; Left = 718; Top = 306; Width = 104; Action = {
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



