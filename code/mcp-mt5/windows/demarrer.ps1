<#
.SYNOPSIS
    Lance un composant du pont, le surveille, et le relance s'il s'arrête.

.DESCRIPTION
    Lancé par une tâche planifiée à l'ouverture de session. Deux raisons
    d'exister plutôt que d'appeler `py` directement depuis la tâche.

    D'abord la trace. Une tâche planifiée n'a pas de console : tout ce que le
    pont écrit sur sa sortie disparaîtrait, et le jour où il refuse de démarrer
    il n'y aurait rien à lire. Ici chaque flux part dans un fichier daté.

    Ensuite la reprise. Un processus qui meurt à trois heures du matin ne doit
    pas laisser le système muet jusqu'au lendemain. La boucle le relance, avec
    une attente qui s'allonge quand il meurt aussitôt — parce qu'un démarrage
    qui échoue en deux secondes est une erreur de configuration, et que marteler
    une erreur de configuration ne fait que remplir le disque.

    Cette boucle rend aussi l'ordre de démarrage inutile : le bot s'arrête de
    lui-même quand le pont n'est pas joignable, donc il réessaiera jusqu'à ce
    qu'il le soit.

.PARAMETER Composant
    `server` pour le pont MCP, `bot` pour le bot Telegram.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File demarrer.ps1 -Composant server
#>

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("server", "bot")]
    [string]$Composant
)

$ErrorActionPreference = "Stop"

# Le script vit dans `windows/`, les modules dans le dossier parent. Aucun
# chemin absolu ici : le dépôt doit pouvoir être cloné n'importe où.
$dossier = Split-Path -Parent $PSScriptRoot
$cible = Join-Path $dossier "$Composant.py"
if (-not (Test-Path $cible)) {
    throw "introuvable : $cible"
}

$journaux = Join-Path $env:LOCALAPPDATA "mcp-mt5"
New-Item -ItemType Directory -Force -Path $journaux | Out-Null

# Redirigé vers un fichier, Python retombe sur l'encodage local de Windows —
# cp1252 ici — tandis que les lignes du lanceur partent en UTF-8. Le fichier
# mélangerait alors deux encodages, et les accents deviendraient illisibles
# précisément là où on vient lire un message d'erreur.
$env:PYTHONIOENCODING = "utf-8"
$env:PYTHONUTF8 = "1"

# Le nom se recalcule à chaque écriture : un processus qui tourne plusieurs
# jours doit changer de fichier à minuit, pas écrire indéfiniment dans celui
# du jour où il a démarré.
function Get-Journal {
    Join-Path $journaux ("{0}-{1}.log" -f $Composant, (Get-Date -Format "yyyy-MM-dd"))
}

function Write-Journal([string]$Texte) {
    $ligne = "{0}  [lanceur] {1}" -f (Get-Date -Format "yyyy-MM-ddTHH:mm:ss"), $Texte
    Add-Content -Path (Get-Journal) -Value $ligne -Encoding UTF8
}

#: Attente après un arrêt immédiat, doublée à chaque fois jusqu'à ce plafond.
$ATTENTE_INITIALE = 5
$ATTENTE_MAX = 300

#: Au-delà de cette durée, le processus est considéré comme ayant réellement
#: tourné : son arrêt est un incident isolé, pas un refus de démarrer.
$DUREE_SAINE = 60

$attente = $ATTENTE_INITIALE

Write-Journal "lanceur demarre pour $Composant ($cible)"

while ($true) {
    $debut = Get-Date
    Write-Journal "demarrage"

    try {
        # La redirection est confiée à `cmd`, et ce n'est pas de la paresse.
        # Sous PowerShell 5.1, `>>` écrit en UTF-16 alors qu'`Add-Content`
        # écrit en UTF-8 : le même fichier deviendrait illisible. Et `2>&1`
        # côté PowerShell transforme la sortie d'erreur d'un programme externe
        # en objets d'erreur, que `$ErrorActionPreference = "Stop"` ferait
        # remonter comme des pannes — or uvicorn écrit tout son journal sur
        # cette sortie-là. `cmd` se contente d'ajouter des octets.
        $fichier = Get-Journal
        $commande = 'py -3.12 "{0}" >> "{1}" 2>&1' -f $cible, $fichier
        & cmd.exe /c $commande
        $code = $LASTEXITCODE
    }
    catch {
        Write-Journal "echec du lancement : $_"
        $code = -1
    }

    $duree = ((Get-Date) - $debut).TotalSeconds
    Write-Journal ("arret apres {0:n0} s, code {1}" -f $duree, $code)

    if ($duree -ge $DUREE_SAINE) {
        $attente = $ATTENTE_INITIALE
    }
    else {
        $attente = [Math]::Min($attente * 2, $ATTENTE_MAX)
    }

    Write-Journal "nouvelle tentative dans $attente s"
    Start-Sleep -Seconds $attente
}
