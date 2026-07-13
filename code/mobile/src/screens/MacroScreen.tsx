// Macro terminal: economic calendar (red/orange) + aggregated news headlines.

import React, { useCallback, useState } from 'react';
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

import type { EcoEvent, NewsItem } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, radius, spacing } from '../theme';

export default function MacroScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [events, setEvents] = useState<EcoEvent[]>([]);
  const [news, setNews] = useState<NewsItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      // Fetch both, but don't let one failing source blank the whole screen.
      const [cal, nw] = await Promise.allSettled([api.getCalendar(), api.getNews()]);
      if (cal.status === 'fulfilled') {
        setEvents([...cal.value].sort((a, b) => a.date.localeCompare(b.date)));
      } else if (cal.reason instanceof ApiError && cal.reason.status === 401) {
        await signOut();
        return;
      }
      if (nw.status === 'fulfilled') setNews(nw.value);

      if (cal.status === 'rejected' && nw.status === 'rejected') {
        setError('Sources macro injoignables (vérifie la connexion du serveur).');
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Erreur inconnue');
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

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color={colors.primary} size="large" />
      </View>
    );
  }

  return (
    <ScrollView
      style={styles.container}
      contentContainerStyle={styles.content}
      refreshControl={
        <RefreshControl refreshing={refreshing} onRefresh={() => { setRefreshing(true); load(); }} tintColor={colors.primary} />
      }
    >
      {error ? <Text style={styles.error}>{error}</Text> : null}

      <Text style={styles.section}>Calendrier éco · cette semaine</Text>
      {events.length === 0 ? (
        <Text style={styles.empty}>Aucun événement rouge/orange.</Text>
      ) : (
        events.map((e, i) => (
          <View key={`${e.title}-${i}`} style={styles.eventRow}>
            <View style={[styles.dot, { backgroundColor: e.impact === 'red' ? colors.red : colors.amber }]} />
            <View style={styles.eventBody}>
              <Text style={styles.eventTitle}>{e.title}</Text>
              <Text style={styles.eventMeta}>
                {e.currency} · {formatDateTime(e.date)}
                {e.forecast ? `  ·  prév. ${e.forecast}` : ''}
                {e.previous ? `  ·  préc. ${e.previous}` : ''}
              </Text>
            </View>
          </View>
        ))
      )}

      <Text style={styles.section}>News marché</Text>
      {news.length === 0 ? (
        <Text style={styles.empty}>Pas de news pour le moment.</Text>
      ) : (
        news.map((n, i) => (
          <Pressable key={`${n.url}-${i}`} style={styles.newsRow} onPress={() => Linking.openURL(n.url)}>
            <View style={styles.sourceChip}>
              <Text style={styles.sourceText}>{n.source}</Text>
            </View>
            <View style={styles.newsBody}>
              <Text style={styles.newsTitle} numberOfLines={3}>{n.title}</Text>
              {n.published_at ? <Text style={styles.newsTime}>{formatDateTime(n.published_at)}</Text> : null}
            </View>
          </Pressable>
        ))
      )}
    </ScrollView>
  );
}

function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleString('fr-FR', {
    weekday: 'short',
    day: '2-digit',
    month: 'short',
    hour: '2-digit',
    minute: '2-digit',
  });
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 2 },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: colors.bg },
  error: { color: colors.red, marginBottom: spacing.sm },
  section: { color: colors.text, fontSize: 16, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.sm },
  empty: { color: colors.textMuted, fontSize: 13 },
  eventRow: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    paddingVertical: spacing.sm,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  dot: { width: 8, height: 8, borderRadius: 4, marginTop: 5, marginRight: spacing.sm },
  eventBody: { flex: 1 },
  eventTitle: { color: colors.text, fontSize: 14, fontWeight: '500' },
  eventMeta: { color: colors.textMuted, fontSize: 12, marginTop: 2 },
  newsRow: {
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
  newsBody: { flex: 1 },
  newsTitle: { color: colors.text, fontSize: 14, lineHeight: 19 },
  newsTime: { color: colors.textMuted, fontSize: 11, marginTop: 2 },
});
