# Packs de iconos

Fuentes de iconos incluidas en hypr-win para personalizar los temas (menú > Temas > Editar el tema actual > Iconos).
Cada fuente está **recortada** a los iconos que usa la interfaz (menús, avisos, widgets y los de estado de la barra y del
panel); las originales completas están en los proyectos.

| Pack | Proyecto | Licencia |
|---|---|---|
| `phosphor`, `phosphor-bold`, `phosphor-fill` | [Phosphor Icons](https://phosphoricons.com) (`@phosphor-icons/web` 2.1.2) | MIT: `LICENSE-phosphor.txt` |
| `lucide` | [Lucide](https://lucide.dev) (`lucide-static` 1.66.0) | ISC: `LICENSE-lucide.txt` |
| `fluent`, `fluent-fill` | [Fluent UI System Icons](https://github.com/microsoft/fluentui-system-icons) (`@fluentui/react-icons` 2.0.345) | MIT: `LICENSE-fluent.txt` |
| `tabler` | [Tabler Icons](https://tabler.io/icons) (`@tabler/icons-webfont` 3.50.0) | MIT: `LICENSE-tabler.txt` |

No se incluye Remix Icon: desde 2026 usa una licencia propia ("Remix Icon License v1.0") que restringe la redistribución
como pack o fuente de iconos, y su `package.json` dice Apache-2.0, así que no está claro.

Se regeneran con `tools/build_icon_packs.py` (ver su cabecera). Para añadir un icono nuevo a la interfaz hay que añadir su
fila a la tabla `ICONS` (o a `STATUS`) de ese script y volver a ejecutarlo; los tests comprueban que todos los packs tienen
todos los glifos que usa el código.
