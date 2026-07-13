// Terms & conditions reached from the burger menu.

import React from 'react';
import { ScrollView, StyleSheet, Text, View } from 'react-native';

import { colors, radius, spacing } from '../theme';

const SECTIONS: Array<{ title: string; body: string }> = [
  {
    title: '1. Objet',
    body:
      "Cette application est un journal de trading personnel, privé et auto-hébergé, destiné à " +
      "l'usage individuel de son propriétaire. Elle sert à enregistrer, visualiser et analyser " +
      'des opérations de trading à des fins de suivi et de progression.',
  },
  {
    title: '2. Pas de conseil financier',
    body:
      "Le contenu de l'application (statistiques, scores, indicateurs macro, actualités, modèles de " +
      'stratégie) est fourni à titre informatif et éducatif uniquement. Il ne constitue pas un conseil ' +
      'en investissement, une recommandation ni une sollicitation. Le trading comporte un risque de ' +
      'perte en capital ; les performances passées ne préjugent pas des performances futures.',
  },
  {
    title: '3. Données',
    body:
      'Les données (trades, comptes, notes) sont stockées sur ton propre serveur/instance. Toi seul en ' +
      'es responsable : sauvegardes, accès et confidentialité. Aucune donnée n’est vendue ni partagée ' +
      'avec des tiers par l’application.',
  },
  {
    title: '4. Sources externes',
    body:
      'Les données de marché, calendrier économique, indicateurs et actualités proviennent de sources ' +
      'tierces publiques. Elles peuvent comporter des erreurs, des retards ou des interruptions, et sont ' +
      'fournies « en l’état », sans garantie d’exactitude ni de disponibilité.',
  },
  {
    title: '5. Responsabilité',
    body:
      'L’application est fournie « telle quelle », sans garantie d’aucune sorte. L’utilisateur est seul ' +
      'responsable des décisions prises sur la base des informations affichées. La responsabilité de ' +
      'l’auteur ne saurait être engagée pour toute perte, directe ou indirecte, liée à son usage.',
  },
  {
    title: '6. Évolutions',
    body:
      'Les fonctionnalités et ces conditions peuvent évoluer. La version la plus récente affichée dans ' +
      'l’application fait foi.',
  },
];

export default function TermsScreen() {
  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.content}>
      <Text style={styles.intro}>
        Conditions d’utilisation — journal de trading personnel. Dernière mise à jour : juillet 2026.
      </Text>
      {SECTIONS.map((s) => (
        <View key={s.title} style={styles.block}>
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
  intro: { color: colors.textMuted, fontSize: 13, lineHeight: 19, marginBottom: spacing.md },
  block: { marginBottom: spacing.md },
  title: { color: colors.text, fontSize: 15, fontWeight: '700', marginBottom: spacing.xs },
  body: { color: colors.textMuted, fontSize: 13, lineHeight: 19 },
});
