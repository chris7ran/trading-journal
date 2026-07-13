// Profile / account screen reached from the burger menu.

import React, { useState } from 'react';
import { ActivityIndicator, Alert, Pressable, ScrollView, Share, StyleSheet, Text, View } from 'react-native';

import type { Trade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, radius, spacing } from '../theme';

const APP_VERSION = '1.0.0';

function csvCell(v: unknown): string {
  const s = v === null || v === undefined ? '' : String(v);
  return `"${s.replace(/"/g, '""')}"`;
}

function tradesToCsv(trades: Trade[]): string {
  const cols: (keyof Trade)[] = [
    'symbol', 'direction', 'open_time', 'close_time', 'open_price', 'close_price',
    'lot_size', 'pnl', 'setup_tag', 'emotion_tag', 'notes',
  ];
  const header = cols.join(',');
  const rows = trades.map((t) => cols.map((c) => csvCell(t[c])).join(','));
  return [header, ...rows].join('\n');
}

export default function ProfileScreen() {
  const api = useApi();
  const { serverUrl, signOut } = useAuth();
  const [exporting, setExporting] = useState(false);

  async function onExport() {
    setExporting(true);
    try {
      const trades = await api.listTrades({ limit: 5000 });
      if (trades.length === 0) {
        Alert.alert('Export', 'Aucun trade à exporter.');
        return;
      }
      const csv = tradesToCsv(trades);
      await Share.share({ title: 'trades.csv', message: csv });
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      Alert.alert('Export', e instanceof Error ? e.message : 'Échec de l’export.');
    } finally {
      setExporting(false);
    }
  }

  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.content}>
      <View style={styles.card}>
        <Row label="Statut" value="Connecté" valueColor={colors.greenBright} />
        <Row label="Serveur" value={serverUrl ?? '—'} />
        <Row label="Version" value={APP_VERSION} />
      </View>

      <Text style={styles.note}>
        Comptes de trading : ajoute et bascule entre tes comptes (funded, démo…) via le
        sélecteur de compte en haut du Dashboard et de l'onglet Coach. La connexion
        multi-utilisateur sera câblée plus tard.
      </Text>

      <Pressable style={[styles.exportBtn, exporting && { opacity: 0.6 }]} onPress={onExport} disabled={exporting}>
        {exporting ? <ActivityIndicator color={colors.primary2} /> : <Text style={styles.exportText}>Exporter mes trades (CSV)</Text>}
      </Pressable>

      <Pressable style={styles.logout} onPress={signOut}>
        <Text style={styles.logoutText}>Déconnexion</Text>
      </Pressable>
    </ScrollView>
  );
}

function Row({ label, value, valueColor }: { label: string; value: string; valueColor?: string }) {
  return (
    <View style={styles.row}>
      <Text style={styles.rowLabel}>{label}</Text>
      <Text style={[styles.rowValue, valueColor ? { color: valueColor } : null]} numberOfLines={1}>{value}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md },
  card: { backgroundColor: colors.card, borderColor: colors.border, borderWidth: 1, borderRadius: radius.lg, paddingHorizontal: spacing.md },
  row: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center', paddingVertical: spacing.md, borderBottomColor: colors.border, borderBottomWidth: StyleSheet.hairlineWidth, gap: spacing.md },
  rowLabel: { color: colors.textMuted, fontSize: 14 },
  rowValue: { color: colors.text, fontSize: 14, fontWeight: '600', flexShrink: 1, textAlign: 'right' },
  note: { color: colors.textMuted, fontSize: 13, lineHeight: 19, marginTop: spacing.md },
  exportBtn: { borderWidth: 1, borderColor: colors.primary, borderRadius: 12, paddingVertical: spacing.md, alignItems: 'center', marginTop: spacing.lg },
  exportText: { color: colors.primary2, fontSize: 15, fontWeight: '600' },
  logout: { backgroundColor: colors.red, borderRadius: 12, paddingVertical: spacing.md, alignItems: 'center', marginTop: spacing.md },
  logoutText: { color: '#fff', fontSize: 16, fontWeight: '700' },
});
