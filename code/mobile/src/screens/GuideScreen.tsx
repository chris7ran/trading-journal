// User guide reached from the burger menu.

import React from 'react';
import { ScrollView, StyleSheet, Text, View } from 'react-native';

import { colors, radius, spacing } from '../theme';

const SECTIONS: Array<{ title: string; body: string }> = [
  {
    title: 'Dashboard',
    body:
      "Vue d'ensemble : P&L net, score de discipline, courbe d'equity et P&L par jour. " +
      'Blocs interactifs — tape une barre pour voir le détail : Trading Health Trends (santé ' +
      'de trading dans le temps), Performance by Day & Time (par créneau horaire), Top Performing ' +
      'Setups et Top Symbols. Le sélecteur en haut filtre par compte, et la barre de filtres par ' +
      'période/instrument.',
  },
  {
    title: 'Journal',
    body:
      'Liste de tes trades et calendrier mensuel des gains/pertes. Tape un jour du calendrier pour ' +
      'sauter au trade correspondant (surligné). Bouton « + Add New » : importer un fichier MT5 ' +
      '(.xlsx ou .csv) ou saisir un trade manuellement. Tape un trade pour l’ouvrir, puis « Modifier ».',
  },
  {
    title: 'Import MT5',
    body:
      'Depuis MetaTrader 5 : clic droit sur l’historique → « Rapport » → enregistre en XLSX (ou CSV). ' +
      'Dans Journal → + Add New → Importer un fichier MT5, choisis le fichier. Les doublons ' +
      '(même ticket) sont ignorés automatiquement.',
  },
  {
    title: 'Setups',
    body:
      'Tes stratégies/patterns et leurs règles. Chaque carte montre l’anneau de win-rate, le nombre ' +
      'de trades, le P&L, la fiabilité et le dernier usage. Tape une carte pour la fiche détaillée ' +
      '(metrics, performance dans le temps, par jour/heure, et champs Description / Target Entry / ' +
      'Target Exit / Stop Loss). Ajoute un modèle prêt à l’emploi (Gold Reversal Sniper, ORB) ou crée ' +
      'le tien. Assigne un setup à un trade depuis sa fiche pour comparer les performances.',
  },
  {
    title: 'Économie',
    body:
      'Calendrier économique de la semaine (impact rouge = fort, orange = moyen) et indicateurs macro ' +
      '(inflation, chômage, PIB) avec mini-courbes. Filtre par actif : USD, EUR, US30, DAX40.',
  },
  {
    title: 'News',
    body: 'Fil d’actualité marché agrégé, avec une indication de sentiment (haussier / baissier / neutre).',
  },
  {
    title: 'Coach',
    body:
      'Analyse de ta discipline : top erreurs à éviter (à partir de ta revue post-trade) et performance ' +
      'par setup. Filtre par compte et par période.',
  },
  {
    title: 'Revue post-trade',
    body:
      'Sur la fiche d’un trade, réponds aux questions (plan suivi ? stop respecté ? pattern valide ? ' +
      'thèse tenue ? sortie maîtrisée ?). Ces réponses alimentent le Coach et les « top erreurs ».',
  },
  {
    title: 'Prop firm',
    body:
      'Sur le Dashboard, la carte Prop firm suit ton objectif, ton drawdown et ta règle de consistance ' +
      'pour le compte sélectionné. Configure les règles via l’icône réglages de la carte.',
  },
];

export default function GuideScreen() {
  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.content}>
      <Text style={styles.intro}>
        Ton journal de trading, privé et auto-hébergé. Voici comment tirer parti de chaque onglet.
      </Text>
      {SECTIONS.map((s) => (
        <View key={s.title} style={styles.card}>
          <Text style={styles.title}>{s.title}</Text>
          <Text style={styles.body}>{s.body}</Text>
        </View>
      ))}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 2 },
  intro: { color: colors.textMuted, fontSize: 14, lineHeight: 20, marginBottom: spacing.md },
  card: { backgroundColor: colors.card, borderColor: colors.border, borderWidth: 1, borderLeftWidth: 3, borderLeftColor: colors.primary, borderRadius: radius.md, padding: spacing.md, marginBottom: spacing.sm },
  title: { color: colors.text, fontSize: 15, fontWeight: '700', marginBottom: spacing.xs },
  body: { color: colors.textMuted, fontSize: 13, lineHeight: 19 },
});
