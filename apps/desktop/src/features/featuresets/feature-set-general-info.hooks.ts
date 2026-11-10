import { useState } from 'react';
import type { FeatureSet } from '@/lib/api/featureSets';

export interface GeneralInfo {
  name: string;
  description: string;
  icon: string;
}

/**
 * Pick the editable general fields off a FeatureSet.
 */
function toGeneralInfo(featureSet: FeatureSet): GeneralInfo {
  return {
    name: featureSet.name,
    description: featureSet.description ?? '',
    icon: featureSet.icon ?? '',
  };
}

/**
 * Editable name, description, and icon with a saved baseline to diff against.
 * Resets to the incoming FeatureSet whenever it changes.
 */
export function useFeatureSetGeneralInfo(featureSet: FeatureSet) {
  const [source, setSource] = useState(featureSet);
  const [values, setValues] = useState<GeneralInfo>(() => toGeneralInfo(featureSet));
  const [baseline, setBaseline] = useState<GeneralInfo>(() => toGeneralInfo(featureSet));

  if (source !== featureSet) {
    setSource(featureSet);
    setValues(toGeneralInfo(featureSet));
    setBaseline(toGeneralInfo(featureSet));
  }

  const hasChanges =
    values.name.trim() !== baseline.name ||
    values.description.trim() !== baseline.description ||
    values.icon.trim() !== baseline.icon;

  /**
   * Update one field.
   */
  const setField = (field: keyof GeneralInfo, value: string) =>
    setValues((prev) => ({ ...prev, [field]: value }));

  /**
   * Adopt the server's saved copy as both the form values and the new baseline.
   */
  const commit = (saved: FeatureSet) => {
    const next = toGeneralInfo(saved);
    setValues(next);
    setBaseline(next);
  };

  return { values, baseline, hasChanges, setField, commit };
}
