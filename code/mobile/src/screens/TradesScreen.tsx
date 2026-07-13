// Journal: list of trades from GET /trades, with pull-to-refresh.

import React, { useCallback, useMemo, useRef, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  FlatList,
  Pressable,
  RefreshControl,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useFocusEffect } from '@react-navigation/native';
import * as DocumentPicker from 'expo-document-picker';
import { CalendarHeatmap } from '../components/CalendarHeatmap';
import { dailyPnlSeries } from '../utils/series';

import type { Trade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, formatPnl, pnlColor, spacing } from '../theme';

export default function TradesScreen({ navigation }: { navigation: any }) {
  const api = useApi();
  const { signOut } = useAuth();
  const [trades, setTrades] = useState<Trade[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const daily = useMemo(() => dailyPnlSeries(trades), [trades]);
  const listRef = useRef<FlatList<Trade>>(null);
  const [highlightId, setHighlightId] = useState<string | null>(null);

  const onDayPress = useCallback(
    (dayKey: string) => {
      const idx = trades.findIndex(
        (t) => (t.close_time ?? t.open_time ?? t.created_at).replace(' ', 'T').slice(0, 10) === dayKey,
      );
      if (idx >= 0) {
        const id = trades[idx].id;
        setHighlightId(id);
        listRef.current?.scrollToIndex({ index: idx, viewPosition: 0.1, animated: true });
        // Clear the highlight after a moment.
        setTimeout(() => setHighlightId((cur) => (cur === id ? null : cur)), 2600);
      }
    },
    [trades],
  );

  const load = useCallback(async () => {
    setError(null);
    try {
      const data = await api.listTrades({ limit: 200 });
      setTrades(data);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Erreur inconnue');
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [api, signOut]);

  // Reload whenever the screen regains focus (e.g. after add/edit).
  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const onRefresh = useCallback(() => {
    setRefreshing(true);
    load();
  }, [load]);

  // Pick an MT5 export (.xlsx or .csv) and upload it to the backend.
  const onImport = useCallback(async () => {
    try {
      const picked = await DocumentPicker.getDocumentAsync({
        type: [
          'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet', // .xlsx
          'text/csv',
          'text/comma-separated-values',
          'application/octet-stream',
          '*/*',
        ],
        copyToCacheDirectory: true,
      });
      if (picked.canceled) return;

      setImporting(true);
      const asset = picked.assets[0];
      const fileResp = await fetch(asset.uri); // read local file
      const blob = await fileResp.blob();
      const summary = await api.importFile(blob);

      Alert.alert(
        'Import terminé',
        `${summary.imported} ajouté(s), ${summary.skipped_duplicates} doublon(s) ignoré(s)` +
          (summary.failed ? `, ${summary.failed} échec(s)` : '') +
          '.',
      );
      await load();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      Alert.alert("Échec de l'import", e instanceof Error ? e.message : 'Erreur inconnue');
    } finally {
      setImporting(false);
    }
  }, [api, load, signOut]);

  // Header "+" : choose import or manual entry.
  const openAdd = useCallback(() => {
    Alert.alert('Ajouter un trade', undefined, [
      { text: 'Importer un fichier MT5', onPress: onImport },
      { text: 'Saisie manuelle', onPress: () => navigation.navigate('TradeForm') },
      { text: 'Annuler', style: 'cancel' },
    ]);
  }, [onImport, navigation]);

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color={colors.primary} size="large" />
      </View>
    );
  }

  return (
    <FlatList
      ref={listRef}
      style={styles.list}
      contentContainerStyle={trades.length === 0 && styles.emptyContainer}
      data={trades}
      keyExtractor={(t) => t.id}
      onScrollToIndexFailed={(info) => {
        listRef.current?.scrollToOffset({ offset: info.averageItemLength * info.index, animated: true });
        setTimeout(() => listRef.current?.scrollToIndex({ index: info.index, viewPosition: 0 }), 300);
      }}
      refreshControl={
        <RefreshControl refreshing={refreshing} onRefresh={onRefresh} tintColor={colors.primary} />
      }
      ListHeaderComponent={
        <View>
          <View style={styles.topBar}>
            <Pressable onPress={openAdd} style={styles.addBtn} accessibilityLabel="Ajouter un trade">
              <Text style={styles.addBtnText}>＋ Add New</Text>
            </Pressable>
          </View>
          {daily.length > 0 ? (
            <View style={styles.calCard}>
              <CalendarHeatmap data={daily} onDayPress={onDayPress} />
            </View>
          ) : null}
          {importing ? (
            <View style={styles.importing}>
              <ActivityIndicator color={colors.primary} />
              <Text style={styles.importText}>Import en cours…</Text>
            </View>
          ) : null}
          {error ? <Text style={styles.error}>{error}</Text> : null}
        </View>
      }
      ListEmptyComponent={
        <Text style={styles.empty}>
          Aucun trade. Importe un CSV MT5 ou ajoute une entrée manuelle.
        </Text>
      }
      renderItem={({ item }) => (
        <Pressable
          style={[styles.row, item.id === highlightId && styles.rowHighlight]}
          onPress={() => navigation.navigate('TradeDetail', { trade: item })}
        >
          <View style={styles.rowLeft}>
            <Text style={styles.symbol}>{item.symbol}</Text>
            <Text style={styles.meta}>{formatDate(item.open_time ?? item.created_at)}</Text>
          </View>
          <View style={styles.rowRight}>
            {item.direction ? (
              <View
                style={[
                  styles.pill,
                  { backgroundColor: item.direction === 'LONG' ? 'rgba(22,199,132,0.16)' : 'rgba(246,70,93,0.16)' },
                ]}
              >
                <Text style={[styles.pillText, { color: item.direction === 'LONG' ? colors.green : colors.red }]}>
                  {item.direction}
                </Text>
              </View>
            ) : null}
            <Text style={[styles.pnl, { color: pnlColor(item.pnl) }]}>{formatPnl(item.pnl)}</Text>
          </View>
        </Pressable>
      )}
    />
  );
}

function formatDate(iso: string): string {
  // Accepts "2026-06-20T09:30:00" or "2026-06-20 09:30:00".
  const d = new Date(iso.replace(' ', 'T'));
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleDateString('fr-FR', { day: '2-digit', month: 'short' });
}

const styles = StyleSheet.create({
  list: { flex: 1, backgroundColor: colors.bg },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: colors.bg },
  emptyContainer: { flexGrow: 1, justifyContent: 'center' },
  row: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.md,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  rowLeft: { flex: 1 },
  rowRight: { flexDirection: 'row', alignItems: 'center', gap: spacing.sm },
  symbol: { color: colors.text, fontSize: 16, fontWeight: '600' },
  meta: { color: colors.textMuted, fontSize: 13, marginTop: 2 },
  pill: { paddingHorizontal: 8, paddingVertical: 3, borderRadius: 8 },
  pillText: { fontSize: 10, fontWeight: '600' },
  pnl: { fontSize: 16, fontWeight: '700', minWidth: 64, textAlign: 'right' },
  importBtn: {
    margin: spacing.md,
    paddingVertical: spacing.sm + 2,
    borderRadius: 8,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.card,
    alignItems: 'center',
  },
  importText: { color: colors.textMuted, fontSize: 14 },
  importing: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingHorizontal: spacing.md, paddingTop: spacing.sm },
  topBar: {
    flexDirection: 'row',
    justifyContent: 'flex-end',
    paddingHorizontal: spacing.md,
    paddingTop: spacing.md,
  },
  addBtn: {
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: colors.green,
    borderRadius: 18,
    paddingHorizontal: 16,
    height: 36,
  },
  addBtnText: { color: '#fff', fontSize: 14, fontWeight: '700' },
  rowHighlight: {
    backgroundColor: 'rgba(124,92,252,0.18)',
    borderLeftWidth: 3,
    borderLeftColor: colors.primary,
  },
  calCard: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: 12,
    padding: spacing.md,
    margin: spacing.md,
    marginBottom: 0,
  },
  error: { color: colors.red, padding: spacing.md },
  empty: { color: colors.textMuted, textAlign: 'center', padding: spacing.lg },
});
