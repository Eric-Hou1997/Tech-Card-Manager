<div align="center">

# Tech Card Manager

[简体中文](../../README.md) | [繁體中文](./README.zh-Hant.md) | [English](./README.en.md) | [Français](./README.fr.md) | [Русский](./README.ru.md) | [日本語](./README.ja.md) | **Español** | [ไทย](./README.th.md)

[![Release](https://img.shields.io/github/v/release/Eric-Hou1997/Tech-Card-Manager?label=release)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Downloads](https://img.shields.io/github/downloads/Eric-Hou1997/Tech-Card-Manager/total?label=downloads)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)

<img src="../../windows/assets/TCM_logo.png" alt="Tech Card Manager" width="220">

Índice NFO de solo lectura y gestión de tarjetas Technical Specifications para Emby Server.

</div>

## Descripción

Tech Card Manager (TCM) analiza en modo de solo lectura los NFO de películas y series elegidos por el usuario, crea su propio índice derivado y muestra mediante una Web Card de Emby las cámaras, objetivos, formatos de captura, sonido, relaciones de aspecto y procesos de producción.

TCM no obtiene datos de IMDb, no escribe NFO y no genera etiquetas. Para crear o mantener esos datos, utiliza la aplicación independiente [IMDb Tech Manager (ITM)](https://github.com/Eric-Hou1997/IMDb-Tech-Manager).

## Versión y plataforma

- Versión estable: [`v4.1.0`](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/tag/v4.1.0)
- Plataforma: Windows x64 Portable
- Archivo: `TCM-v4.1.0-Windows-x64-EXE.zip`
- Ejecutable estable dentro del ZIP: `Tech-Card-Manager.exe`
- La Release incluye SHA-256, instrucciones, registro de cambios y paquetes de idioma

## Funciones principales

- Carpetas, índices, búsqueda, filtros y actualización separados para películas y series
- Inspector NFO de solo lectura y diagnóstico XML con rutas completas
- Instalación, actualización, eliminación y estado de la Web Card de Emby
- Instancia única de Windows, bandeja del sistema, inicio de sesión y modo silencioso
- Diseño adaptativo del título, panel, herramientas NFO y vista dividida según el contenido
- Actualización que distingue proxy/red, límites de GitHub, recursos ausentes y fallos de descarga

## Idiomas

El chino simplificado, el chino tradicional y el inglés (Estados Unidos) están integrados en el EXE Portable. El francés, ruso, japonés, español y tailandés se distribuyen como paquetes separados en la Release `v4.1.0` y solo se cargan después de descargarlos y verificarlos.

El idioma del nuevo registro se fija al iniciar la tarea. Los registros anteriores, NFO, índices, copias y datos del usuario no se reescriben. Un idioma de Emby no instalado o no compatible vuelve al chino simplificado. Las claves `Camera`, `Sound mix` y la estructura JSON permanecen estables; solo se traduce la presentación.

## Límites de seguridad

- Los NFO siempre son de solo lectura para TCM; la indexación no cambia sus bytes ni marcas de tiempo.
- La Web Card solo mantiene archivos Web de Emby verificados y usa copia externa, bloqueo, CAS, diario y reemplazo atómico.
- La migración de componentes antiguos y las operaciones administrativas muestran un plan y exigen confirmación explícita.
- Si no puede verificarse la ruta, propiedad o copia, la operación se detiene de forma segura.

## Interfaz

![TCM Manager](../images/card-manager.PNG)

![Tarjeta Technical Specifications de Emby](../images/media-library-card.png)

## Instalación y actualización

Extrae completamente el ZIP, ejecuta `Tech-Card-Manager.exe`, configura por separado las carpetas de películas y series, crea el índice y sigue las indicaciones para la Web Card. TCM debe permanecer activo mientras se usa la tarjeta y puede minimizarse a la bandeja.

La versión Portable no reemplaza automáticamente el EXE activo. Sal por completo desde la bandeja, extrae la nueva versión y reemplaza el programa conservando `data`, `logs`, `backup`, `runtime` y `updates`.

## Hoja de ruta

Completado: Windows x64 Portable, índice NFO de solo lectura, espacios separados de películas/series, Web Card segura, ciclo de vida de bandeja, registro multilingüe y cinco paquetes, actualización compatible con proxy y diseño adaptativo medido.

En curso: más pruebas reales Windows/Emby/PowerShell 5.1/UAC, compatibilidad con DOM de Emby y tipos multimedia, recuperación de componentes antiguos, actualización Portable y otras plataformas.

## Desarrollo y licencia

Lee [`AGENTS.md`](../../AGENTS.md) antes de contribuir. El diseño de los paquetes está en [`docs/language-packs.md`](../language-packs.md).

Proyecto bajo [Apache License 2.0](../../LICENSE). IMDb, Emby y las demás marcas pertenecen a sus propietarios. Este proyecto no está afiliado, autorizado ni respaldado por IMDb.com, Inc. o Emby LLC.
