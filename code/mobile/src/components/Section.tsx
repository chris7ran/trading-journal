// Unified collapsible section used across every tab, with a colored chevron.
// `flush` = no horizontal padding (for screens whose content is already padded).

import React, { useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';

import { colors, spacing } from '../theme';

export function Section({
  title,
  children,
  defaultOpen = true,
  flush = false,
}: {
  title: string;
  children: React.ReactNode;
  defaultOpen?: boolean;
  flush?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <View style={styles.wrap}>
      <Pressable
        style={[styles.head, { paddingHorizontal: flush ? 0 : spacing.md }]}
        onPress={() => setOpen((o) => !o)}
        hitSlop={6}
      >
        <Text style={styles.title}>{title}</Text>
        <Ionicons name={open ? 'chevron-down' : 'chevron-forward'} size={20} color={colors.primary2} />
      </Pressable>
      {open ? <View>{children}</View> : null}
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: { marginTop: spacing.md },
  head: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', marginBottom: spacing.sm },
  title: { color: colors.text, fontSize: 16, fontWeight: '600' },
});
