<#
.SYNOPSIS
    Prépare un ou plusieurs dossiers de terminal MT5 portables, à partir d'un
    terminal existant.

.DESCRIPTION
    Chaque compte veut son propre dossier : le paquet Python `MetaTrader5`
    n'accepte qu'une connexion de terminal par processus, et le pont lance un
    processus par dossier.

    La copie laisse volontairement derrière elle l'état du terminal source —
    identifiants enregistrés, historique, journaux, bases de cotations. Deux
    raisons. La première est le poids : ces dossiers pèsent souvent plusieurs
    gigaoctets pour quelques centaines de mégaoctets de programme. La seconde
    compte davantage : un dossier portable recopié se souvient de sa dernière
    session, et un terminal qui se reconnecterait tout seul au compte d'origine
    pousserait des trades vers le mauvais compte du journal. Le pont refuse de
    démarrer dans ce cas, mais autant ne pas créer la situation.

    Le script ne lance ni ne connecte rien : la connexion se fait à la main,
    une fois par compte, parce qu'elle demande un mot de passe.

.PARAMETER Alias
    Un ou plusieurs alias. Le dossier porte le même nom, et c'est celui à
    inscrire dans `accounts.toml`.

.PARAMETER Source
    Terminal existant à recopier.

.PARAMETER Racine
    Où créer les dossiers.

.PARAMETER Audit
    Racine des journaux d'audit. Un sous-dossier par alias y est créé : deux
    comptes qui partageraient un fichier mélangeraient leurs lignes.

.PARAMETER Force
    Autorise l'écrasement d'un dossier déjà présent.

.EXAMPLE
    .\preparer-terminal.ps1 -Alias goat-2step, goat-paylater
#>

param(
    [Parameter(Mandatory = $true)]
    [string[]]$Alias,
    [string]$Source = 'C:\MT5\xm-demo',
    [string]$Racine = 'C:\MT5',
    [string]$Audit = 'F:\audit',
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$executable = Join-Path $Source "terminal64.exe"
if (-not (Test-Path $executable)) {
    throw "terminal64.exe introuvable dans $Source — indiquez un terminal existant avec -Source"
}

#: Dossiers d'état, inutiles dans une copie neuve et parfois énormes.
$DOSSIERS_EXCLUS = @("logs", "bases", "history", "Tester", "Templates")

#: `accounts.dat` contient les comptes enregistrés du terminal source.
$FICHIERS_EXCLUS = @("accounts.dat", "*.log")

$prepares = @()

foreach ($nom in $Alias) {
    $cible = Join-Path $Racine $nom

    if ((Test-Path $cible) -and -not $Force) {
        $contenu = Get-ChildItem $cible -Force -ErrorAction SilentlyContinue
        if ($contenu) {
            Write-Warning "$cible existe deja et n'est pas vide — ignore (utilisez -Force pour ecraser)"
            continue
        }
    }

    Write-Host "copie vers $cible ..."
    $options = @(
        $Source, $cible, "/E", "/NFL", "/NDL", "/NJH", "/NJS", "/NP",
        "/XD"
    ) + $DOSSIERS_EXCLUS + @("/XF") + $FICHIERS_EXCLUS

    & robocopy @options | Out-Null
    # robocopy rend 0 a 7 pour un succes ; 8 et au-dela sont de vraies erreurs.
    if ($LASTEXITCODE -ge 8) {
        throw "echec de la copie vers $cible (robocopy $LASTEXITCODE)"
    }

    $dossierAudit = Join-Path $Audit $nom
    New-Item -ItemType Directory -Force -Path $dossierAudit | Out-Null

    $taille = (Get-ChildItem $cible -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
    Write-Host ("  {0:n0} Mo, audit dans {1}" -f $taille, $dossierAudit)
    $prepares += $nom
}

if (-not $prepares) {
    Write-Host "`nrien a preparer."
    return
}

Write-Host "`nConnectez maintenant chaque terminal, un par un :"
foreach ($nom in $prepares) {
    Write-Host ("  & '{0}' /portable" -f (Join-Path (Join-Path $Racine $nom) "terminal64.exe"))
}

Write-Host @"

Dans chaque terminal : Fichier > Ouvrir un compte, cherchez le serveur par son
nom, puis connectez-vous avec le login et le mot de passe du compte.

Si le serveur reste introuvable, c'est que le courtier ne le publie pas dans
l'annuaire MetaQuotes : prenez son installateur depuis votre espace client, il
embarque le fichier de serveur. Le dossier prepare ici reste utilisable, il
suffit d'y installer par-dessus.

Verifiez ensuite que le terminal affiche bien le bon numero de compte avant de
l'inscrire dans accounts.toml : le pont refuse de demarrer si le login
declare ne correspond pas a celui reellement connecte.
"@
