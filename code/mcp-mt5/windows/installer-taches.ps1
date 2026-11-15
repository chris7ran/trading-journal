<#
.SYNOPSIS
    Déclare les deux tâches planifiées qui démarrent le pont et le bot.

.DESCRIPTION
    À lancer **une fois**, dans un PowerShell administrateur, depuis la VM
    d'exécution. Les tâches sont réentrantes : relancer ce script les remplace
    sans en créer de doublon.

    Les deux tâches s'exécutent à l'ouverture de session, pas au démarrage de
    la machine, et seulement quand l'utilisateur est connecté. Ce n'est pas un
    détail de confort : le terminal MetaTrader 5 est une application graphique
    et n'a pas de fenêtre dans la session 0, celle des services. Pour qu'un
    redémarrage remette réellement le système en route, il faut donc que la
    session s'ouvre seule — voir la section « Démarrage automatique » du
    README, qui explique comment et ce que ça coûte.

    Une tâche planifiée relit l'environnement utilisateur au lancement. Elle
    verra donc toujours la valeur à jour de `TELEGRAM_BOT_TOKEN`, là où une
    fenêtre PowerShell ouverte avant une modification garde l'ancienne — le
    piège qui a coûté une demi-heure le 14/11.

.PARAMETER Utilisateur
    Compte dont l'ouverture de session déclenche les tâches. Par défaut, celui
    qui exécute ce script.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File installer-taches.ps1
#>

param(
    [string]$Utilisateur = "$env:USERDOMAIN\$env:USERNAME"
)

$ErrorActionPreference = "Stop"

$lanceur = Join-Path $PSScriptRoot "demarrer.ps1"
if (-not (Test-Path $lanceur)) {
    throw "introuvable : $lanceur"
}

#: Le bot interroge le pont dès son démarrage. Il survit à son absence — le
#: lanceur le relancera — mais autant lui laisser une longueur d'avance et
#: s'épargner des échecs inutiles dans les journaux.
$DELAI_BOT = "PT30S"

function Install-Tache {
    param(
        [string]$Nom,
        [string]$Composant,
        [string]$Delai
    )

    $arguments = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}" -Composant {1}' -f $lanceur, $Composant
    $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $arguments

    $declencheur = New-ScheduledTaskTrigger -AtLogOn -User $Utilisateur
    if ($Delai) {
        $declencheur.Delay = $Delai
    }

    # Sans limite de durée : ces tâches ne se terminent jamais, et le défaut
    # de trois jours les tuerait silencieusement un mardi matin.
    $parametres = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) `
        -MultipleInstances IgnoreNew

    Register-ScheduledTask -TaskName $Nom -Action $action -Trigger $declencheur `
        -Settings $parametres -RunLevel Limited -Force | Out-Null

    Write-Host "tache installee : $Nom"
}

Install-Tache -Nom "mcp-mt5-pont" -Composant "server" -Delai $null
Install-Tache -Nom "mcp-mt5-bot" -Composant "bot" -Delai $DELAI_BOT

Write-Host ""
Write-Host "Journaux : $(Join-Path $env:LOCALAPPDATA 'mcp-mt5')"
Write-Host ""
Write-Host "Demarrer maintenant sans attendre une ouverture de session :"
Write-Host '  Start-ScheduledTask -TaskName mcp-mt5-pont'
Write-Host '  Start-ScheduledTask -TaskName mcp-mt5-bot'
Write-Host ""
Write-Host "Arreter les lanceurs, puis les processus qu'ils ont laisses :"
Write-Host '  Stop-ScheduledTask -TaskName mcp-mt5-bot, mcp-mt5-pont'
Write-Host '  Get-CimInstance Win32_Process -Filter "Name like ''py%''" | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }'
