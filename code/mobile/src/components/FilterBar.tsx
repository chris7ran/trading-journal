// Compact filters: a segmented period control + an instrument dropdown.

import React, { useState } from 'react';
import { Modal, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';

import { colors, radius, spacing } from '../theme';
import { Filters, PERIOD_LABELS, Period } from '../utils/filters';

const PERIODS: Period[] = ['all', '30d', '90d', 'ytd'];

export function FilterBar({
  value,
  symbols,
  onChange,
}: {
  value: Filters;
  symbols: string[];
  onChange: (f: Filters) => void;
}) {
  const [open, setOpen] = useState(false);

  return (
    <View style={styles.container}>
      <View style={styles.segment}>
        {PERIODS.map((p) => {
          const active = value.period === p;
          return (
            <Pressable
              key={p}
              style={[styles.seg, active && styles.segActive]}
              onPress={() => onChange({ ...value, period: p })}
            >
              <Text style={[styles.segText, active && styles.segTextActive]}>{PERIOD_LABELS[p]}</Text>
            </Pressable>
          );
        })}
      </View>

      {symbols.length > 0 ? (
        <Pressable style={styles.dropdown} onPress={() => setOpen(true)}>
          <Ionicons name="pricetag-outline" size={15} color={colors.primary2} />
          <Text style={styles.dropdownText} numberOfLines={1}>{value.symbol ?? 'Tous les instruments'}</Text>
          <Ionicons name="chevron-down" size={16} color={colors.textMuted} />
        </Pressable>
      ) : null}

      <Modal visible={open} transparent animationType="fade" onRequestClose={() => setOpen(false)}>
        <Pressable style={styles.backdrop} onPress={() => setOpen(false)}>
          <Pressable style={styles.sheet} onPress={() => {}}>
            <Text style={styles.sheetTitle}>Instrument</Text>
            <ScrollView>
              <Row
                label="Tous les instruments"
                selected={value.symbol === null}
                onPress={() => { onChange({ ...value, symbol: null }); setOpen(false); }}
              />
              {symbols.map((s) => (
                <Row
                  key={s}
                  label={s}
                  selected={value.symbol === s}
                  onPress={() => { onChange({ ...value, symbol: s }); setOpen(false); }}
                />
              ))}
            </ScrollView>
          </Pressable>
        </Pressable>
      </Modal>
    </View>
  );
}

function Row({ label, selected, onPress }: { label: string; selected: boolean; onPress: () => void }) {
  return (
    <Pressable style={styles.row} onPress={onPress}>
      <Text style={styles.rowText}>{label}</Text>
      {selected ? <Ionicons name="checkmark" size={18} color={colors.primary} /> : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  container: { paddingHorizontal: spacing.md, paddingTop: spacing.sm },
  segment: { flexDirection: 'row', backgroundColor: colors.card2, borderRadius: radius.md, padding: 3 },
  seg: { flex: 1, paddingVertical: spacing.sm - 1, alignItems: 'center', borderRadius: radius.md - 3 },
  segActive: { backgroundColor: colors.primary },
  segText: { color: colors.textMuted, fontSize: 13, fontWeight: '600' },
  segTextActive: { color: '#fff' },
  dropdown: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.sm,
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.md,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    marginTop: spacing.sm,
  },
  dropdownText: { flex: 1, color: colors.text, fontSize: 14, fontWeight: '500' },
  backdrop: { flex: 1, backgroundColor: 'rgba(0,0,0,0.5)', justifyContent: 'flex-end' },
  sheet: { backgroundColor: colors.surface, borderTopLeftRadius: radius.xl, borderTopRightRadius: radius.xl, padding: spacing.md, maxHeight: '60%' },
  sheetTitle: { color: colors.textMuted, fontSize: 13, marginBottom: spacing.sm },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingVertical: spacing.md,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  rowText: { color: colors.text, fontSize: 15, fontWeight: '500' },
});
