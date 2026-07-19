// Read-only trade detail. Editing (notes, tags) comes in a later sprint.

import React, { useState } from 'react';
import { ActivityIndicator, Alert, Image, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

import { ApiError } from '../api/client';
import type { Trade } from '../api/types';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, formatPnl, pnlColor, spacing } from '../theme';

export default function TradeDetailScreen({ route, navigation }: { route: any; navigation: any }) {
  const trade: Trade = route.params.trade;
  const api = useApi();
  const { signOut } = useAuth();
  const [deleting, setDeleting] = useState(false);

  function confirmDelete() {
    Alert.alert(
      'Supprimer le trade',
      'Cette action est définitive. Continuer ?',
      [
        { text: 'Annuler', style: 'cancel' },
        {
          text: 'Supprimer',
          style: 'destructive',
          onPress: async () => {
            setDeleting(true);
            try {
              await api.deleteTrade(trade.id);
              navigation.goBack(); // the trades list refreshes on focus
            } catch (e) {
              if (e instanceof ApiError && e.status === 401) {
                await signOut();
                return;
              }
              setDeleting(false);
              Alert.alert('Échec de la suppression', e instanceof Error ? e.message : 'Erreur inconnue');
            }
          },
        },
      ],
    );
  }

  const rows: Array<[string, string]> = [
    ['Symbole', trade.symbol],
    ['Direction', trade.direction ?? '—'],
    ['Ouverture', trade.open_time ?? '—'],
    ['Clôture', trade.close_time ?? '—'],
    ['Prix entrée', num(trade.open_price)],
    ['Prix sortie', num(trade.close_price)],
    ['Lot', num(trade.lot_size)],
    ['Commission', num(trade.commission)],
    ['Swap', num(trade.swap)],
    ['Setup', trade.setup_tag ?? '—'],
    ['Émotion', trade.emotion_tag ?? '—'],
    ['Ticket MT5', trade.mt5_ticket ?? '—'],
  ];

  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.content}>
      <View style={styles.pnlCard}>
        <Text style={styles.pnlLabel}>P&L</Text>
        <Text style={[styles.pnlValue, { color: pnlColor(trade.pnl) }]}>
          {formatPnl(trade.pnl)}
        </Text>
      </View>

      {rows.map(([label, value]) => (
        <View key={label} style={styles.row}>
          <Text style={styles.label}>{label}</Text>
          <Text style={styles.value}>{value}</Text>
        </View>
      ))}

      {hasReview(trade) ? (
        <View style={styles.reviewBlock}>
          <Text style={styles.reviewTitle}>Revue</Text>
          <ReviewRow label="Plan suivi" v={trade.followed_plan} />
          <ReviewRow label="Stop loss respecté" v={trade.respected_sl} />
          <ReviewRow label="Pattern bien identifié" v={trade.pattern_valid} />
          <ReviewRow label="Thèse / logique validée" v={trade.thesis_worked} />
          <ReviewRow label="Sortie maîtrisée" v={trade.good_exit} />
        </View>
      ) : null}

      {trade.notes ? (
        <View style={styles.notes}>
          <Text style={styles.label}>Notes</Text>
          <Text style={styles.notesText}>{trade.notes}</Text>
        </View>
      ) : null}

      {trade.screenshot_url ? (
        <View style={styles.notes}>
          <Text style={styles.label}>Capture</Text>
          <Image source={{ uri: trade.screenshot_url }} style={styles.screenshot} resizeMode="contain" />
        </View>
      ) : null}

      <Pressable
        style={[styles.deleteBtn, deleting && { opacity: 0.6 }]}
        onPress={confirmDelete}
        disabled={deleting}
      >
        {deleting ? (
          <ActivityIndicator color={colors.red} />
        ) : (
          <Text style={styles.deleteText}>Supprimer le trade</Text>
        )}
      </Pressable>
    </ScrollView>
  );
}

function hasReview(t: Trade): boolean {
  return [t.followed_plan, t.respected_sl, t.pattern_valid, t.thesis_worked, t.good_exit].some(
    (v) => v !== null && v !== undefined,
  );
}

function ReviewRow({ label, v }: { label: string; v: boolean | null }) {
  const mark = v === null || v === undefined ? '—' : v ? '✓' : '✗';
  const color = v === null || v === undefined ? colors.textMuted : v ? colors.green : colors.red;
  return (
    <View style={styles.row}>
      <Text style={styles.label}>{label}</Text>
      <Text style={[styles.value, { color }]}>{mark}</Text>
    </View>
  );
}

function num(v: number | null): string {
  return v === null || v === undefined ? '—' : String(v);
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md },
  pnlCard: {
    backgroundColor: colors.card,
    borderRadius: 12,
    padding: spacing.lg,
    alignItems: 'center',
    marginBottom: spacing.md,
  },
  pnlLabel: { color: colors.textMuted, fontSize: 13 },
  pnlValue: { fontSize: 32, fontWeight: '800', marginTop: spacing.xs },
  row: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    paddingVertical: spacing.sm,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  label: { color: colors.textMuted, fontSize: 14 },
  value: { color: colors.text, fontSize: 14, fontWeight: '500' },
  reviewBlock: { marginTop: spacing.md },
  reviewTitle: { color: colors.text, fontSize: 16, fontWeight: '600', marginBottom: spacing.xs },
  notes: { marginTop: spacing.md },
  notesText: { color: colors.text, fontSize: 14, marginTop: spacing.xs, lineHeight: 20 },
  screenshot: { width: '100%', height: 240, borderRadius: 8, marginTop: spacing.xs, backgroundColor: colors.card },
  deleteBtn: {
    marginTop: spacing.lg,
    paddingVertical: spacing.md,
    borderRadius: 10,
    borderWidth: 1,
    borderColor: colors.red,
    alignItems: 'center',
  },
  deleteText: { color: colors.red, fontSize: 15, fontWeight: '600' },
});
