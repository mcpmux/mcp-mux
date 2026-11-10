import { useCallback, useState } from 'react';
import type { FeatureSet } from '@/lib/api/featureSets';
import type { ServerFeature } from '@/lib/api/serverFeatures';

interface SelectionSeed {
  selectedIds: Set<string>;
  surfacedIds: Set<string>;
}

/**
 * Initial selection for a FeatureSet: its include-mode feature members.
 */
function seedSelection(featureSet: FeatureSet): SelectionSeed {
  const selectedIds = new Set<string>();
  const surfacedIds = new Set<string>();
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
    () => seedSelection(featureSet).selectedIds
  );
  const [surfacedIds, setSurfacedIds] = useState(
    () => seedSelection(featureSet).surfacedIds
  );
  const [isEdited, setIsEdited] = useState(false);

  if (source.featureSet !== featureSet || source.features !== features) {
    const seed = seedSelection(featureSet);
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
    markSaved,
  };
}
