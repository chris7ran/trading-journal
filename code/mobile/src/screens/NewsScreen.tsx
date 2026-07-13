// Market news feed, filterable by asset (keyword match), tap to open in browser.

import React, { useCallback, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  Linking,
  Pressable,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useFocusEffect } from '@react-navigation/native';

import type { NewsItem } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, radius, spacing } from '../theme';
import { AssetBar } from '../components/AssetBar';
import { Asset, matchNews } from '../utils/assets';

export default function NewsScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [news, setNews] = useState<NewsItem[]>([]);
  const [asset, setAsset] = useState<Asset>('all');
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      setNews(await api.getNews());
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'News injoignables.');
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [api, signOut]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const filtered = useMemo(() => news.filter((n) => matchNews(n, asset)), [news, asset]);

  const mood = useMemo(() => {
    const bull = filtered.filter((n) => n.sentiment === 'bullish').length;
    const bear = filtered.filter((n) => n.sentiment === 'bearish').length;
    const tot = bull + bear;
    return { bull, bear, tot, pct: tot ? bull / tot : 0.5 };
  }, [filtered]);

  return (
    <View style={styles.container}>
      <AssetBar value={asset} onChange={setAsset} />
      {loading ? (
        <View style={styles.centered}>
          <ActivityIndicator color={colors.primary} size="large" />
        </View>
      ) : (
        <ScrollView
          style={styles.scroll}
          contentContainerStyle={styles.content}
          refreshControl={
            <RefreshControl refreshing={refreshing} onRefresh={() => { setRefreshing(true); load(); }} tintColor={colors.primary} />
          }
        >
          {error ? <Text style={styles.error}>{error}</Text> : null}

          {filtered.length > 0 ? (
            <View style={styles.mood}>
              <View style={styles.moodHead}>
                <Text style={styles.moodTitle}>Humeur du marché</Text>
                <Text style={styles.moodPct}>{mood.tot ? `${Math.round(mood.pct * 100)}% haussier` : '—'}</Text>
              </View>
              <View style={styles.moodTrack}>
                <View style={[styles.moodFill, { width: `${Math.round(mood.pct * 100)}%` }]} />
              </View>
              <Text style={styles.moodSub}>
                {mood.bull} haussières · {mood.bear} baissières · {filtered.length - mood.tot} neutres
              </Text>
            </View>
          ) : null}

          {filtered.length === 0 ? (
            <Text style={styles.empty}>Aucune news pour ce filtre.</Text>
          ) : (
            filtered.map((n, i) => (
              <Pressable key={`${n.url}-${i}`} style={styles.row} onPress={() => Linking.openURL(n.url)}>
                <View style={[styles.sdot, { backgroundColor: sentColor(n.sentiment) }]} />
                <View style={styles.sourceChip}>
                  <Text style={styles.sourceText}>{n.source}</Text>
                </View>
                <View style={styles.body}>
                  <Text style={styles.title} numberOfLines={3}>{n.title}</Text>
                  {n.published_at ? <Text style={styles.time}>{formatDateTime(n.published_at)}</Text> : null}
                </View>
              </Pressable>
            ))
          )}
        </ScrollView>
      )}
    </View>
  );
}

function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleString('fr-FR', { weekday: 'short', day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' });
}

function sentColor(s: NewsItem['sentiment']): string {
  return s === 'bullish' ? colors.green : s === 'bearish' ? colors.red : colors.textMuted;
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  scroll: { flex: 1 },
  centered: { flex: 1, justifyContent: 'center' },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 2 },
  error: { color: colors.red, marginBottom: spacing.sm },
  empty: { color: colors.textMuted, fontSize: 13 },
  mood: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.md,
    padding: spacing.md,
    marginBottom: spacing.md,
  },
  moodHead: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center', marginBottom: spacing.sm },
  moodTitle: { color: colors.text, fontSize: 14, fontWeight: '600' },
  moodPct: { color: colors.textMuted, fontSize: 13 },
  moodTrack: { height: 8, borderRadius: 4, backgroundColor: colors.red, overflow: 'hidden' },
  moodFill: { height: 8, borderRadius: 4, backgroundColor: colors.green },
  moodSub: { color: colors.textMuted, fontSize: 11, marginTop: spacing.xs },
  sdot: { width: 8, height: 8, borderRadius: 4, marginTop: 6, marginRight: spacing.sm },
  row: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    paddingVertical: spacing.sm,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  sourceChip: {
    backgroundColor: colors.card2,
    borderRadius: radius.md,
    paddingHorizontal: spacing.sm,
    paddingVertical: 2,
    marginRight: spacing.sm,
    marginTop: 2,
  },
  sourceText: { color: colors.primary2, fontSize: 10, fontWeight: '600' },
  body: { flex: 1 },
  title: { color: colors.text, fontSize: 14, lineHeight: 19 },
  time: { color: colors.textMuted, fontSize: 11, marginTop: 2 },
});
