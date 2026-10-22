export interface RetryOptions {
  attempts?: number;
  shouldRetry?: (error: unknown) => boolean;
}

/** Retry an asynchronous operation with a caller-supplied transient-error policy. */
export async function retry<T>(
  operation: (attempt: number) => Promise<T>,
  options: RetryOptions = {},
): Promise<T> {
  const attempts = options.attempts ?? 3;
  if (!Number.isInteger(attempts) || attempts < 1) {
    throw new RangeError('Attempts must be a positive integer');
  }
  for (let attempt = 1; ; attempt++) {
    try {
      return await operation(attempt);
    } catch (error) {
      if (attempt === attempts || options.shouldRetry?.(error) === false) {
        throw error;
      }
    }
  }
}
