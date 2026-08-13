import { useEffect, useRef } from 'react';
import { useAppDispatch, useAppSelector } from '../../app/hooks';
import {
  persistColorLabDraft,
  setSuppressRemote,
  type ColorLabDraftSnapshot,
} from '../../app/slices/colorLabSlice';
import { emitColorLabDraftChanged } from '../../shared/ipc';

/**
 * Persist + broadcast Color Lab draft so sidebar and floating window stay in sync.
 */
export function useColorLabDraftSync(): void {
  const dispatch = useAppDispatch();
  const name = useAppSelector((s) => s.colorLab.name);
  const colors = useAppSelector((s) => s.colorLab.colors);
  const extractMethod = useAppSelector((s) => s.colorLab.extractMethod);
  const extractCount = useAppSelector((s) => s.colorLab.extractCount);
  const chromaWeight = useAppSelector((s) => s.colorLab.chromaWeight);
  const contrastWeight = useAppSelector((s) => s.colorLab.contrastWeight);
  const remoteEpoch = useAppSelector((s) => s.colorLab.remoteEpoch);

  const skipFirst = useRef(true);
  const lastRemoteEpoch = useRef(remoteEpoch);

  useEffect(() => {
    const draft: ColorLabDraftSnapshot = {
      name,
      colors,
      extractMethod,
      extractCount,
      chromaWeight,
      contrastWeight,
    };

    if (skipFirst.current) {
      skipFirst.current = false;
      persistColorLabDraft(draft);
      lastRemoteEpoch.current = remoteEpoch;
      return;
    }

    persistColorLabDraft(draft);

    if (remoteEpoch !== lastRemoteEpoch.current) {
      lastRemoteEpoch.current = remoteEpoch;
      return;
    }

    dispatch(setSuppressRemote(true));
    void emitColorLabDraftChanged(draft).finally(() => {
      window.setTimeout(() => dispatch(setSuppressRemote(false)), 80);
    });
  }, [colors, dispatch, extractCount, extractMethod, name, chromaWeight, contrastWeight, remoteEpoch]);
}
