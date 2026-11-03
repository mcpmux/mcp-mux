import { useCallback, useState } from 'react';
import type { FeatureSet } from '@/lib/api/featureSets';
import type { ServerFeature } from '@/lib/api/serverFeatures';

interface SelectionSeed {
  selectedIds: Set<string>;
  surfacedIds: Set<string>;
}

/**
 * Initial selection for a FeatureSet: its include-mode feature members, or in auto mode
 * every available feature (that is what the set grants).
 */
function seedSelection(featureSet: FeatureSet, features: ServerFeature[]): SelectionSeed {
  const selectedIds = new Set<string>();
  const surfacedIds = new Set<string>();
  if (featureSet.auto_include) {
    features.filter((f) => f.is_available).forEach((f) => selectedIds.add(f.id));
    return { selectedIds, surfacedIds };
  }
  featureSet.members?.forEach((member) => {
    if (member.member_type !== 'feature' || member.mode !== 'include') {
      return;
    }
    selectedIds.add(member.member_id);
    if (member.surfaced) {
      surfacedIds.add(member.member_id);
    }
  });
  return { selectedIds, surfacedIds };
}

/**
 * Selected and surfaced feature ids for a FeatureSet, reseeded whenever the set or the
 * loaded features change, plus whether the user has edited them since.
 */
export function useFeatureSetSelection(featureSet: FeatureSet, features: ServerFeature[]) {
  const [source, setSource] = useState({ featureSet, features });
  const [selectedIds, setSelectedIds] = useState(
    () => seedSelection(featureSet, features).selectedIds
  );
  const [surfacedIds, setSurfacedIds] = useState(
    () => seedSelection(featureSet, features).surfacedIds
  );
  const [isEdited, setIsEdited] = useState(false);

  if (source.featureSet !== featureSet || source.features !== features) {
    const seed = seedSelection(featureSet, features);
    setSource({ featureSet, features });
    setSelectedIds(seed.selectedIds);
    setSurfacedIds(seed.surfacedIds);
    setIsEdited(false);
  }

  /**
   * Select or deselect the given ids. Deselecting also un-surfaces them.
   */
  const setIdsSelected = useCallback((ids: string[], isSelected: boolean) => {
    setIsEdited(true);
    setSelectedIds((prev) => {
      const next = new Set(prev);
      ids.forEach((id) => (isSelected ? next.add(id) : next.delete(id)));
      return next;
    });
    if (isSelected) {
      return;
    }
    setSurfacedIds((prev) => {
      const next = new Set(prev);
      ids.forEach((id) => next.delete(id));
      return next;
    });
  }, []);

  /**
   * Flip one feature's selection.
   */
  const toggleFeature = useCallback(
    (id: string) => setIdsSelected([id], !selectedIds.has(id)),
    [selectedIds, setIdsSelected]
  );

  /**
   * Flip whether an included feature is promoted into the client's list.
   */
  const toggleSurfaced = useCallback((id: string) => {
    setIsEdited(true);
    setSurfacedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  /**
   * Replace the selection without marking it edited (auto mode just granted it).
   */
  const replaceSelection = useCallback((ids: string[]) => {
    setSelectedIds(new Set(ids));
    setIsEdited(false);
  }, []);

  /**
   * Mark the current selection as persisted.
   */
  const markSaved = useCallback(() => setIsEdited(false), []);

  return {
    selectedIds,
    surfacedIds,
    isEdited,
    toggleFeature,
    toggleSurfaced,
    setIdsSelected,
    replaceSelection,
    markSaved,
  };
}
