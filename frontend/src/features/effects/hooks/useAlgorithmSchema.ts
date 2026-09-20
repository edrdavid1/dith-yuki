import { useEffect, useState } from 'react';
import { getAlgorithmSchema, type ParamField } from '../../../shared/ipc/registry';

export interface AlgorithmSchemaResult {
  /** `null` while the schema request is in flight (Req 5.2). */
  schema: ParamField[] | null;
  error: boolean;
}

/**
 * Loads `param_schema` for a registered algorithm.
 * `schema` is `null` while loading; `error` is set if the id is unknown.
 */
export function useAlgorithmSchema(id: string | null): AlgorithmSchemaResult {
  const [schema, setSchema] = useState<ParamField[] | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (id == null || id === '') {
      setSchema(null);
      setError(false);
      return;
    }

    let cancelled = false;
    setSchema(null);
    setError(false);
    void getAlgorithmSchema(id)
      .then((fields) => {
        if (!cancelled) {
          setSchema(fields);
          setError(false);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setSchema(null);
          setError(true);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [id]);

  return { schema, error };
}
