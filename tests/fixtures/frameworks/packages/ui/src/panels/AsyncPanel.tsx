export type Result = { state: 'ready'; value: string } | { state: 'error'; value: string };

export async function loadMessage(load: () => Promise<string>): Promise<Result> {
  try {
    const value = await load();
    if (value.trim() === '') {
      return { state: 'error', value: 'Empty response' };
    }
    return { state: 'ready', value };
  } catch (error) {
    if (error instanceof Error) {
      return { state: 'error', value: error.message };
    }
    return { state: 'error', value: 'Unknown failure' };
  }
}

export function AsyncPanel({ result }: { result?: Result }) {
  if (!result) {
    return <p role="status">Loading…</p>;
  }
  if (result.state === 'error') {
    return <p role="alert">{result.value}</p>;
  }
  return <output>{result.value}</output>;
}
