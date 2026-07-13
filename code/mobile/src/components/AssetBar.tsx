// Asset selector chips (Tout / USD / EUR / US30 / DAX40).

import React from 'react';
import { Pressable, ScrollView, StyleSheet, Text } from 'react-native';

import { colors, radius, spacing } from '../theme';
import { Asset, ASSETS, ASSET_LABELS } from '../utils/assets';

export function AssetBar({ value, onChange }: { value: Asset; onChange: (a: Asset) => void }) {
  return (
    <ScrollView
      horizontal
      showsHorizontalScrollIndicator={false}
      style={styles.bar}
      contentContainerStyle={styles.row}
    >
      {ASSETS.map((a) => {
        const active = value === a;
        return (
          <Pressable key={a} style={[styles.chip, active ? styles.chipActive : null]} onPress={() => onChange(a)}>
            <Text style={[styles.chipText, active ? styles.chipTextActive : null]}>{ASSET_LABELS[a]}</Text>
          </Pressable>
        );
      })}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  // flexGrow:0 stops the horizontal ScrollView from expanding vertically and
  // stretching the chips inside a flex column parent.
  bar: { flexGrow: 0, alignSelf: 'stretch' },
  row: { gap: spacing.sm, paddingHorizontal: spacing.md, paddingVertical: spacing.sm, alignItems: 'center' },
  chip: {
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm - 2,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.card,
  },
  chipActive: { backgroundColor: colors.primary, borderColor: colors.primary },
  chipText: { color: colors.textMuted, fontSize: 13, fontWeight: '500' },
  chipTextActive: { color: '#fff' },
});
