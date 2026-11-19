# btree — better tree

Klon `tree` w Rust, nastawiony na czytelność drzewka w terminalu.

```
btree/           50.3 KiB
├─ src/          26.6 KiB
│  ├─ cli.rs      2.7 KiB
│  ├─ main.rs     3.8 KiB
│  ├─ render.rs   7.7 KiB
│  ├─ scan.rs     4.7 KiB
│  ├─ style.rs      959 B
│  ╰─ tree.rs     6.8 KiB
├─ tests/         8.5 KiB
│  ╰─ cli.rs      8.5 KiB
├─ Cargo.lock    11.0 KiB
├─ Cargo.toml       371 B
╰─ README.md      3.8 KiB

2 directories, 10 files, 50.3 KiB total
```

## Czym różni się od `tree`

- **Katalogi najpierw**, potem pliki; sortowanie naturalne i bez rozróżniania wielkości liter (`file2` przed `file10`).
- **Kolory**: katalogi, pliki wykonywalne, symlinki (`link → cel`, zepsute na czerwono). Linie drzewa, podpowiedzi i dotfiles są przygaszone, żeby nazwy się wyróżniały. Kolory wyłączają się same, gdy wyjście nie jest terminalem albo ustawiono `NO_COLOR`.
- **Respektuje `.gitignore`** (także `.ignore`, globalny gitignore i `.git/info/exclude`), również poza repozytorium. Dotfiles i `.git` są ukryte domyślnie.
- **Zwijanie łańcuchów**: katalog zawierający tylko jeden podkatalog jest pokazany jako jedna ścieżka (`src/main/java/`), jak na GitHubie.
- **Limit wpisów na katalog** (domyślnie 50): reszta jest streszczona linią `… 120 more (3 directories, 117 files)`.
- **Limit głębokości mówi, co ukrył**: `node_modules/ … 1204 entries` zamiast pustego katalogu.
- **Kolumna rozmiarów** (`-s`): wyrównana do prawej, katalogi pokazują łączny rozmiar zawartości.
- **Katalogi bez dostępu** są oznaczone w drzewku (`secret/ (cannot read)`) zamiast zasypywać stderr.
- `btree` zamiast `.` pokazuje prawdziwą nazwę bieżącego katalogu.

## Instalacja

```sh
cargo install --path .
```

## Użycie

```sh
btree                     # bieżący katalog
btree -L 2 src docs       # dwa poziomy, kilka ścieżek
btree -s --sort size      # rozmiary, największe pierwsze
btree -P '*.rs' -P '*.toml'   # tylko pasujące pliki (i katalogi, które do nich prowadzą)
btree -I node_modules -I target   # pomiń nazwy pasujące do globu
btree -a --no-ignore      # wszystko: dotfiles i pliki z .gitignore
btree -d                  # tylko katalogi
btree -n 0                # bez limitu wpisów na katalog
btree --no-collapse       # bez zwijania a/b/c/
btree --ascii             # tylko znaki ASCII
btree --color always | less -R
```

| Flaga | Opis |
|---|---|
| `-a, --all` | Pokaż ukryte pliki (`.git` zawsze pomijany) |
| `--no-ignore` | Nie respektuj `.gitignore` / `.ignore` |
| `-d, --dirs-only` | Tylko katalogi |
| `-L, --level N` | Maksymalna głębokość |
| `-P, --pattern GLOB` | Pokaż tylko pliki pasujące do globu (można powtarzać) |
| `-I, --exclude GLOB` | Pomiń pliki i katalogi pasujące do globu (można powtarzać) |
| `-s, --size` | Kolumna rozmiarów, sumy dla katalogów |
| `--sort name\|size` | Kolejność wewnątrz katalogu |
| `-n, --max-entries N` | Maks. wpisów na katalog, `0` = bez limitu (domyślnie 50) |
| `--no-collapse` | Nie zwijaj łańcuchów pojedynczych katalogów |
| `--ascii` | Linie drzewa w ASCII |
| `--color auto\|always\|never` | Kolory |
| `--no-summary` | Bez linii podsumowania |

Kod wyjścia jest `1`, gdy ścieżka nie istnieje albo któregoś katalogu nie dało się odczytać.

## Rozwój

```sh
cargo test
cargo clippy --all-targets
```

Struktura:

- `src/scan.rs`: odczyt systemu plików (crate `ignore`) do drzewa `Node`
- `src/tree.rs`: czyste transformacje drzewa: filtrowanie, rozmiary, sortowanie, limit głębokości, zwijanie łańcuchów
- `src/render.rs`: zamiana drzewa na tekst: linie, etykiety, kolumna rozmiarów, podsumowanie
- `src/style.rs`: kolory ANSI
- `src/cli.rs`: argumenty (`clap`)
- `tests/cli.rs`: testy end-to-end na tymczasowych katalogach
