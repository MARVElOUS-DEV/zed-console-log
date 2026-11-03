// Put the caret on a binding or reference, then open code actions.
export function summarize(values: number[]) {
    const first =
        values[0];

    for (const value of values) {
        consume(value);
    }

    try {
        consume(first);
    } catch (error) {
        consume(error);
    }

    return first;
}

declare function consume(value: unknown): void;
