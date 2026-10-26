# Item and folder IDs

IDs are 13 characters of uppercase base 36, for example `MVP1BZ832CIJ1`.

## The timestamp prefix (observed)

The first 8 characters are the creation time in milliseconds, written in base 36:

| ID | First 8 characters | Decoded | Item's `modificationTime` |
|---|---|---|---|
| `MVP1BZ832CIJ1` | `MVP1BZ83` | 1793005755411 | 1793005755411 |
| `MVP1BZ8496KO7` | `MVP1BZ84` | 1793005755412 | 1793005755412 |
| `MVP1JTFJI1HWU` | `MVP1JTFJ` | 1793006121151 | 1793006121151 |

Because of this prefix, sorting IDs as strings also sorts items by creation time, as long as the prefix stays 8 characters. That holds until the year 2059.

## The suffix

The last 5 characters tell apart items created in the same millisecond. Whether they are random or a counter is still being checked against `app.asar`.
