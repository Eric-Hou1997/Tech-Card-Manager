<div align="center">

# Tech Card Manager

[简体中文](../../README.md) | [繁體中文](./README.zh-Hant.md) | [English](./README.en.md) | [Français](./README.fr.md) | [Русский](./README.ru.md) | [日本語](./README.ja.md) | **Español** | [ไทย](./README.th.md)

<p align="center">

[![Versión](https://img.shields.io/github/v/release/Eric-Hou1997/Tech-Card-Manager?label=versión)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Descargas](https://img.shields.io/github/downloads/Eric-Hou1997/Tech-Card-Manager/total?label=descargas)](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)
[![Estrellas](https://img.shields.io/github/stars/Eric-Hou1997/Tech-Card-Manager?style=flat&logo=github)](https://github.com/Eric-Hou1997/Tech-Card-Manager/stargazers)
[![PR bienvenidas](https://img.shields.io/badge/PRs-bienvenidas-brightgreen.svg)](https://github.com/Eric-Hou1997/Tech-Card-Manager/pulls)

</p>

<img src="../../windows/assets/TCM_logo.png" alt="Tech Card Manager" width="220">

**Herramienta de gestión de tarjetas de especificaciones técnicas e integración con bibliotecas Emby.**

Indexación NFO de solo lectura y gestión de tarjetas Technical Specifications para **Emby Server**.

</div>

---

## 🎬 Acerca del proyecto

**Tech Card Manager (TCM)** incorpora las **Technical Specifications** ya existentes en los archivos NFO a la experiencia real de navegación de Emby.

La mayoría de las bibliotecas multimedia ya presentan correctamente:

* Título
* Reparto
* Año
* Resolución
* Códec de vídeo
* Códec de audio
* Información de flujo como HDR / Dolby Vision

Sin embargo, la información de producción siguiente no suele mostrarse de forma completa y estructurada:

* Qué cámaras se utilizaron
* Qué objetivos se utilizaron
* Qué formatos de captura en película o digital se utilizaron
* Qué procesos cinematográficos intervinieron
* Qué formatos de sonido se utilizaron
* Qué relaciones de aspecto se utilizaron
* Cómo se masterizó y presentó la obra

TCM examina en **modo de solo lectura** los NFO de películas y series configurados por el usuario, construye su propio índice derivado y presenta las especificaciones en las páginas de detalle mediante la tarjeta web de Emby.

Los objetivos principales de TCM son:

* Incorporar las Technical Specifications existentes a la navegación por la biblioteca
* Mantener los NFO multimedia estrictamente como archivos de solo lectura
* No apropiarse del flujo de metadatos existente del usuario
* Gestionar y actualizar por separado las bibliotecas de películas y series
* Mantener de forma segura la tarjeta web Emby Technical Specifications
* Proporcionar una interfaz Windows, integración con la bandeja y un servicio local aptos para uso prolongado
* Hacer observables y verificables los estados, errores y operaciones de mantenimiento importantes
* Incluir chino simplificado, chino tradicional e inglés (Estados Unidos), con paquetes vinculados a la versión para francés, ruso, japonés, español y tailandés

---

## ✨ Funciones principales

### 📚 Indexación NFO de solo lectura

TCM puede leer las carpetas de películas y series configuradas por el usuario y extraer de los NFO las Technical Specifications junto con otros datos de solo lectura necesarios para la presentación e identificación.

Los NFO multimedia son **fuentes de datos de solo lectura** para TCM.

TCM no:

* Modifica archivos NFO
* Reorganiza automáticamente archivos NFO
* «Repara» automáticamente archivos NFO
* Escribe Technical Specifications
* Genera ni elimina etiquetas
* Modifica la propiedad de las etiquetas
* Cambia el contenido original de los NFO

TCM mantiene por separado los datos del índice.

Los metadatos multimedia originales permanecen bajo el control del usuario y de las herramientas ya utilizadas en su flujo de trabajo.

---

### 🖥️ Tarjeta web Emby Technical Specifications

TCM convierte las especificaciones indexadas en datos adecuados para su presentación en la biblioteca y los integra en Emby mediante una tarjeta web.

Las funciones actuales incluyen:

* Generación de tarjetas Technical Specifications
* Presentación de tarjetas Technical Specifications
* Servicio de recursos de la tarjeta
* Integración con la interfaz web de Emby
* Instalación de la tarjeta web
* Actualización de la tarjeta web
* Eliminación de la tarjeta web
* Detección del estado de la tarjeta web
* Detección de compatibilidad de componentes antiguos
* Flujos obligatorios de copia de seguridad y recuperación

El mantenimiento de la tarjeta web y los datos NFO multimedia son dos dominios operativos totalmente separados.

TCM puede mantener los archivos de integración web de Emby, pero **no modifica NFO multimedia como parte de ese proceso**.

El Administrador puede alternar al instante entre chino simplificado, chino tradicional e inglés (Estados Unidos) sin recargar ni borrar el estado actual de la interfaz. El francés, ruso, japonés, español y tailandés son recursos separados de la versión de GitHub `v4.1.0` y solo se cargan después de descargarlos y verificarlos. La tarjeta web Emby tiene un registro de idiomas independiente; los idiomas Emby no instalados o no compatibles recurren al chino simplificado. Las claves Technical Specs como `Camera` y `Sound mix`, y la estructura de datos subyacente, nunca cambian con el idioma de presentación.

---

### 🎬 Gestión separada de películas y series

Las películas y series son espacios multimedia separados en TCM.

Tienen:

* Raíces de biblioteca independientes
* Índices independientes
* Búsquedas independientes
* Filtros independientes
* Navegación independiente
* Ámbitos de actualización independientes
* Presentación de estado independiente

Por ejemplo:

```text
Películas
  ↓
Actualizar biblioteca actual
  ↓
Examinar solo directorios de películas
```

Actualizar Películas no examina automáticamente los directorios de Series.

Del mismo modo, actualizar Series no vuelve a examinar automáticamente Películas.

---

### 🔎 Navegación e inspección del índice

El Administrador TCM no se limita a iniciar el servicio. También permite navegar por el índice multimedia ya construido.

Actualmente puede mostrar:

* Número de títulos indexados
* Número total de NFO
* Estado de caché / índice
* Errores de análisis XML
* Estado de acceso a la biblioteca
* Directorios de películas y series
* Technical Specifications
* Etiquetas técnicas y otros datos de solo lectura
* Rutas NFO
* Tareas actuales
* Estados de error

Cuando se produce un error, TCM conserva tanta información de diagnóstico útil como sea posible, como título, tipo de medio, ruta NFO y tarea afectados.

---

### 🪟 Permanencia en Windows y bandeja del sistema

La implementación oficial actual está destinada a:

**Windows x64**

Al iniciar Tech Card Manager, el Administrador y el servicio local se ejecutan conjuntamente.

Tras minimizar la ventana, TCM puede continuar funcionando en la bandeja del sistema.

Las funciones actuales incluyen:

* Ejecución de instancia única
* Minimización a la bandeja
* Restauración del Administrador desde la bandeja
* Inicio después de iniciar sesión
* Minimización silenciosa a la bandeja al iniciarse con la sesión
* Gestión del estado del servicio
* Salida explícita de la aplicación
* Limpieza de recursos al salir

Al salir completamente de TCM, también se detiene el servicio local.

---

## 🖼️ Vista previa de la interfaz

### Interfaz del Administrador

La interfaz del Administrador permite inspeccionar:

* Estado del servicio
* Estadísticas del índice
* Espacios de películas / series
* Directorios multimedia
* Technical Specifications
* Estado de la tarea actual
* Información de errores

<div align="center">

<img src="../images/card-manager.PNG" alt="Interfaz del Administrador Tech Card Manager" width="700">

</div>

---

### Configuración y mantenimiento

El área de configuración permite administrar:

* Raíces de la biblioteca de películas
* Raíces de la biblioteca de series
* Comportamiento al arrancar
* Inicio después de iniciar sesión
* Inicio silencioso
* Intervalo de actualización
* Mantenimiento de la tarjeta web
* Comprobación de actualizaciones
* Otros ajustes de la aplicación

<div align="center">

<img src="../images/media-etting.PNG" alt="Configuración de Tech Card Manager" width="700">

</div>

---

## 🎞️ Muestra de resultados

### Página de detalles de película de Emby

Las Technical Specifications pueden aparecer como una tarjeta independiente en las páginas de detalles de películas de Emby.

La tarjeta puede presentar datos como:

* Cámaras
* Objetivos
* Captura en película / digital
* Proceso cinematográfico
* Laboratorio
* Relación de aspecto
* Mezcla de sonido
* Formato de copia cinematográfica
* Formato de máster / presentación
* Otras Technical Specifications

<div align="center">

<img src="../images/media-library-card.png" alt="Tarjeta de biblioteca Emby de Tech Card Manager" width="700">

</div>

---

### Página de detalles de serie de Emby

Las series utilizan un flujo independiente de indexación y presentación.

Las Technical Specifications a nivel de serie pueden mostrarse en las páginas de Emby correspondientes.

> 📷 **Marcador para captura del resultado**
>
> Archivo sugerido:
>
> `../images/emby-series-card.png`

<!--
<div align="center">

<img src="../images/emby-series-card.png" alt="Tarjeta de serie Emby de Tech Card Manager" width="700">

</div>
-->

---

### Detalle de la tarjeta Technical Specifications

Esta sección puede mostrar el resultado visual completo de la tarjeta Technical Specifications, con elementos como:

* Cámaras
* Objetivos
* Formatos de película
* Formatos de captura digital
* Procesos cinematográficos
* Formatos de sonido
* Relaciones de aspecto
* Formatos de máster
* Formatos de presentación

> 📷 **Marcador para captura del resultado**
>
> Archivo sugerido:
>
> `../images/technical-specs-card-detail.png`

<!--
<div align="center">

<img src="../images/technical-specs-card-detail.png" alt="Tarjeta Technical Specifications de Tech Card Manager" width="700">

</div>
-->

---

## 🔄 Flujo de trabajo principal

```text
NFO multimedia
    ↓
Exploración y análisis de solo lectura
    ↓
Índice derivado de TCM
    ↓
Servicio local / Recursos de tarjeta
    ↓
Tarjeta web Emby
    ↓
Technical Specifications en la biblioteca multimedia
```

TCM no vuelve a escribir los resultados del índice en los NFO multimedia.

TCM puede entenderse como un adaptador de solo lectura situado entre:

```text
Capa de datos NFO
    ↓
   TCM
    ↓
Capa de presentación Emby
```

Lee las especificaciones existentes, construye su propio índice de presentación y entrega la información a la interfaz de la biblioteca.

---

## 🔗 Relación con IMDb Tech Manager (ITM)

TCM e [**IMDb Tech Manager (ITM)**](https://github.com/Eric-Hou1997/IMDb-Tech-Manager) son dos herramientas independientes.

Trabajan conjuntamente en torno al mismo flujo de **Technical Specifications**.

### 📦 IMDb Tech Manager (ITM)

ITM se ocupa principalmente de producir y mantener los datos de origen, entre ellos:

* Obtención de IMDb Technical Specifications
* Estructuración de Technical Specifications
* Normalización de Technical Specifications
* Gestión NFO
* Escritura de Technical Specifications
* Generación de etiquetas técnicas
* Procesamiento semántico asistido por IA
* Corrección manual
* Procesamiento por lotes
* Mantenimiento de metadatos

---

### 🖥️ Tech Card Manager (TCM)

TCM se ocupa principalmente de la lectura, indexación y presentación posteriores, y de la integración con la biblioteca:

* Leer NFO existentes en modo de solo lectura
* Construir un índice derivado de Technical Specifications
* Administrar la tarjeta web Emby
* Presentar las especificaciones en páginas de la biblioteca

Juntos pueden formar un flujo completo:

```text
IMDb
  ↓
IMDb Tech Manager (ITM)
  ↓
NFO / Technical Specifications
  ↓
Tech Card Manager (TCM)
  ↓
Tarjeta Emby Technical Specifications
```

TCM **no requiere ITM**.

Puede utilizarse cualquier otra fuente de datos siempre que el NFO contenga Technical Specifications compatibles que TCM pueda reconocer.

---

## 🚫 Límites del producto

Tech Card Manager mantiene deliberadamente unos límites estrictos de responsabilidad.

TCM **no**:

* Extrae datos de IMDb
* Modifica NFO multimedia
* Escribe Technical Specifications
* Genera Technical Tags
* Elimina etiquetas
* Modifica etiquetas del usuario
* Ejecuta IA
* Administra prompts
* Registra el uso de tokens de IA
* Registra costes de API de IA
* Se apropia de metadatos NFO
* Migra la propiedad de NFO

Estas responsabilidades pertenecen a herramientas de gestión de datos como IMDb Tech Manager.

TCM se centra en una sola cosa:

**Leer los datos existentes con seguridad y presentarlos de forma fiable.**

---

## 🔒 Seguridad de datos e integración web

### Los NFO permanecen en modo de solo lectura

Durante:

* Exploración
* Indexación
* Actualización
* Búsqueda
* Presentación

TCM debe dejar sin cambios el contenido de los NFO multimedia.

Un NFO que no se pueda analizar se registra como error, en lugar de repararse automáticamente.

---

### Mantenimiento recuperable de la tarjeta web

Instalar, actualizar o eliminar archivos web de Emby es una operación totalmente separada de los datos NFO multimedia.

Estas operaciones de mantenimiento están diseñadas para:

* Confirmar el destino exacto
* Crear una copia de seguridad
* Verificar que la copia se pueda recuperar
* Construir el resultado modificado completo
* Conservar el comportamiento BOM / de finales de línea necesario
* Verificar el resultado al terminar
* Revertir en caso de fallo

Las operaciones que requieren privilegios de administrador se realizan explícitamente mediante Windows UAC.

---

### Migración cuidadosa de componentes antiguos

TCM conserva la detección de compatibilidad para ciertos componentes antiguos, Web Patches previos y rastros de instalaciones históricas.

Para operaciones con efectos secundarios como:

* Eliminar componentes antiguos
* Terminar procesos
* Sustituir Web Patches
* Limpiar archivos históricos

el flujo previsto es:

```text
Identificar destino
    ↓
Mostrar plan de mantenimiento
    ↓
Confirmación del usuario
    ↓
Revalidar destino
    ↓
Ejecutar
    ↓
Verificar resultado
```

Si TCM no puede determinar con fiabilidad la propiedad de un componente antiguo, se detiene para no arriesgar una eliminación insegura.

---

## 🧩 Arquitectura actual

La implementación actual de Windows se compone principalmente de:

```text
GUI Windows / Integración nativa
          +
        Núcleo Go
          +
      Web UI local
          +
   Motor PowerShell
          +
Bandeja / Integración con navegador
          ↓
     Tarjeta web Emby
```

El repositorio se organiza por **producto**, no permanentemente por sistema operativo.

Actualmente es compatible con Windows x64 y se prevén implementaciones para más sistemas operativos.

---

## 💻 Entorno de ejecución actual

La plataforma mantenida actualmente es:

**Windows x64**

El producto y flujo de Release actuales se centran en entornos como:

* Windows x64
* Windows PowerShell 5.1
* Windows UAC
* Bandeja del sistema de Windows
* Carga mediante navegador
* Web UI de Emby Server

Importante:

**Compilar correctamente el código fuente no demuestra el comportamiento en la plataforma real.**

Las siguientes funciones aún requieren validación en entornos Windows / Emby reales:

* UAC
* Ciclo de vida de la bandeja
* Inicio de sesión
* Carga del navegador
* Comportamiento del DOM de Emby
* Instalación de la tarjeta web
* Eliminación de la tarjeta web
* Recuperación de la tarjeta web
* Limpieza de recursos tras salir de la aplicación

---

## 📦 Instalación y uso

### 1. Descargar

Ve a:

[**GitHub Releases →**](https://github.com/Eric-Hou1997/Tech-Card-Manager/releases)

El proyecto no publica un `.exe` independiente sin empaquetar como recurso de Release.

El paquete oficial actual es:

```text
TCM-v4.1.0-Windows-x64-EXE.zip
```

---

### 2. Extraer completamente el ZIP

Primero extrae todo el ZIP en un directorio fijo.

Después ejecuta:

```text
Tech-Card-Manager.exe
```

No ejecutes la aplicación directamente desde el archivo comprimido.

---

### 3. Configurar las bibliotecas multimedia

Después del primer inicio, configura las raíces necesarias en el Administrador.

Las raíces de películas y series se configuran por separado.

Por ejemplo:

```text
Películas
  └── D:\Movies

Series
  └── D:\TV
```

TCM lee los NFO correspondientes de estos directorios.
También puede descubrir automáticamente los directorios de biblioteca desde Emby Server.

---

### 4. Construir el índice

Actualiza la biblioteca correspondiente al espacio actual de películas o series.

```text
NFO
 ↓
Analizar
 ↓
Índice derivado
```

El proceso no escribe los datos del índice en los NFO.

---

### 5. Configurar la tarjeta web Emby

Sigue el estado y las instrucciones del Administrador para instalar o mantener la tarjeta web Emby.

Windows puede solicitar privilegios de administrador cuando sea necesario modificar archivos web de Emby.

---

### 6. Mantener TCM en ejecución

TCM proporciona el servicio local que la tarjeta web utiliza para acceder al índice y los recursos relacionados.

Por ello, TCM debe seguir ejecutándose mientras se use la tarjeta Technical Specifications.

El Administrador puede minimizarse a la bandeja y no necesita permanecer visible en el escritorio.

---

## 🔄 Actualización

TCM puede consultar las Releases oficiales de GitHub desde la página de configuración.

La compilación Portable actual **no reemplaza automáticamente el EXE en ejecución**.

Flujo de actualización recomendado:

```text
Buscar nueva versión
    ↓
Abrir GitHub Release
    ↓
Descargar nuevo ZIP
    ↓
Salir completamente de TCM desde la bandeja
    ↓
Extraer la nueva versión
    ↓
Sustituir archivos del programa
    ↓
Conservar directorio de datos / configuración existentes
    ↓
Reiniciar
    ↓
Verificar estado de ejecución
```

La Release también proporciona:

```text
TCM-v4.1.0-Windows-x64-EXE-SHA256SUMS.txt
```

para verificar la integridad del paquete.

---

## 🤖 Desarrollo con Coding Agents

El repositorio incluye:

[**`AGENTS.md` →**](../../AGENTS.md)

Es uno de los puntos principales de contexto para Coding Agents como Codex al trabajar con Tech Card Manager.

Documenta:

* Identidad del producto
* Límites del repositorio
* Separación de responsabilidades entre TCM e ITM
* Reglas NFO de solo lectura
* Límites de indexación de Technical Specifications
* Reglas de seguridad de la tarjeta web
* Reglas de identificadores de compatibilidad antigua
* Reglas del ciclo de vida de Windows
* Restricciones de UAC y mantenimiento administrativo
* Requisitos de pruebas
* Límites de Release
* Cambios que no deben realizarse

Flujo de desarrollo recomendado:

```text
Fork / Clone
        ↓
El Coding Agent lee AGENTS.md
        ↓
Leer el código fuente y las pruebas pertinentes
        ↓
Confirmar los límites del producto TCM
        ↓
Analizar las rutas funcionales afectadas
        ↓
Crear un plan de implementación
        ↓
Modificar el código
        ↓
Ejecutar las pruebas
        ↓
Validar en Windows / Emby real cuando sea necesario
        ↓
Enviar Pull Request
```

El repositorio pretende proporcionar:

```text
Código fuente
  +
Conocimiento de arquitectura
  +
Restricciones de diseño
  +
Métodos de prueba
  +
Contexto de Agent
```

Esto reduce el riesgo de que los desarrolladores o Coding Agents rompan accidentalmente los límites existentes del producto al modificarlo.

---

## 🚧 Estado actual

Tech Card Manager se encuentra actualmente:

**Código abierto · En desarrollo activo**

Este repositorio se mantiene como producto independiente desde:

**v4.0.0**

La rama de desarrollo predeterminada es:

```text
main
```

Actualmente disponible:

* Código fuente público completo
* GUI Portable para Windows x64
* Release v4.1.0
* Archivo de suma de comprobación SHA-256
* Indexación NFO de solo lectura
* Tarjeta web Emby Technical Specifications
* Espacios de películas / series separados
* Integración con la bandeja de Windows
* Inicio con la sesión
* Inicio silencioso
* Comprobación de actualizaciones
* Suite básica de pruebas
* Scripts de compilación de Release
* `AGENTS.md`
* Licencia Apache 2.0

---

## 🗺️ Hoja de ruta

### Completado

* [x] Establecer un repositorio independiente para Tech Card Manager
* [x] Mantener públicamente el código fuente desde `v4.0.0`
* [x] GUI Portable para Windows x64
* [x] Espacios multimedia separados para películas / series
* [x] Indexación NFO de solo lectura
* [x] Índice derivado de Technical Specifications
* [x] Integración de la tarjeta web Emby
* [x] Integración con la bandeja del sistema
* [x] Ciclo de vida de instancia única
* [x] Inicio con la sesión
* [x] Inicio silencioso en bandeja
* [x] Comprobación de actualizaciones de GitHub Release
* [x] Establecer pruebas básicas de regresión
* [x] Establecer el flujo de compilación de Release
* [x] Publicar la primera versión pública `v4.0.0`
* [x] Publicar el registro de localización `v4.1.0` y los paquetes vinculados a versión
* [x] Completar las interfaces integradas en chino simplificado, chino tradicional e inglés (Estados Unidos)
* [x] Publicar paquetes en francés, ruso, japonés, español y tailandés
* [x] Fijar el idioma de registro de las tareas nuevas conservando registros históricos, índices y bytes NFO
* [x] Distinguir fallos de proxy/red, limitación de GitHub, recurso ausente y descarga
* [x] Completar diseños adaptables medidos por contenido para cabecera, panel y barra NFO

### En curso

* [ ] Mejorar la presentación de la tarjeta Emby Technical Specifications
* [ ] Mejorar la compatibilidad entre tipos de medios
* [ ] Mejorar la compatibilidad con distintas estructuras de páginas Emby
* [ ] Mejorar la compatibilidad entre versiones de Web UI / DOM de Emby
* [ ] Mejorar la localización de errores del índice
* [ ] Mejorar la recuperación de errores
* [ ] Mejorar la migración de componentes antiguos
* [ ] Mejorar la reversión de componentes antiguos
* [ ] Añadir más pruebas de regresión reales en Windows / Emby
* [ ] Mejorar la visualización de estado del Administrador
* [ ] Mejorar la UX de configuración
* [ ] Mejorar la experiencia de actualización Portable
* [ ] Seguir mejorando `AGENTS.md`
* [ ] Seguir mejorando el contexto de Coding Agent
* [ ] Explorar otros sistemas operativos manteniendo el límite de solo lectura de TCM

La hoja de ruta seguirá evolucionando según el desarrollo del proyecto y la experiencia de uso real.

---

## 🐛 Incidencias

Si encuentras un problema reproducible o tienes una solicitud de función bien definida, abre un Issue:

[**GitHub Issues →**](https://github.com/Eric-Hou1997/Tech-Card-Manager/issues)

Cuando sea posible, incluye:

* Versión de Tech Card Manager
* Versión de Emby Server
* Versión de Windows
* Tipo de medio
* Pasos de reproducción
* Información de error mostrada por el Administrador
* Ruta NFO, eliminando datos privados cuando proceda
* Si interviene UAC
* Si interviene la tarjeta web
* Si interviene la migración de componentes antiguos

Esta información puede ayudar a localizar el problema en la cadena:

```text
NFO
 ↓
Índice
 ↓
Servicio
 ↓
Navegador
 ↓
DOM de Emby
 ↓
Renderizado de tarjeta
```

---

## 🤝 Contribuciones

Tech Card Manager es de código abierto.

Se aceptan contribuciones como:

* Forks
* Revisión e investigación del código fuente
* Correcciones de errores
* Mejoras de funciones
* Mejoras de pruebas
* Mejoras de UI / UX
* Mejoras de compatibilidad con Emby
* Mejoras de documentación
* Pull Requests

Antes de modificar el código, lee:

[**`AGENTS.md` →**](../../AGENTS.md)

En particular, conserva los límites del producto al modificar:

* Exploración NFO
* Análisis NFO
* Indexación de Technical Specifications
* Tarjeta web Emby
* Modificación de archivos web
* Recuperación de archivos web
* Compatibilidad de componentes antiguos
* Windows UAC
* Integración con la bandeja
* Ciclo de vida de la aplicación
* Actualizaciones y Releases

**La regla de solo lectura de los NFO multimedia de TCM es una restricción de diseño fundamental.**

---

## 📄 Licencia

Tech Card Manager se distribuye bajo:

**Licencia Apache 2.0**

Licencia completa:

[**LICENSE →**](../../LICENSE)

Documentos adicionales del proyecto:

* [NOTICE](../../NOTICE)
* [PRIVACY.md](../legal/PRIVACY.es.md)
* [SECURITY.md](../../SECURITY.md)
* [TERMS.md](../legal/TERMS.es.md)

Autor: **侯雁泽**

---

## ⚠️ Aviso legal

Tech Card Manager es un proyecto de código abierto desarrollado de forma independiente.

Este proyecto **no está afiliado oficialmente, autorizado ni respaldado por Emby, IMDb o ninguna otra plataforma de terceros**.

Todos los nombres, marcas, datos y servicios de terceros pertenecen a sus respectivos propietarios.

TCM no es responsable del origen ni del estado de licencia de los datos Technical Specifications de terceros.

Los usuarios son responsables de que sus metadatos multimedia, los datos de terceros y el uso de los servicios relacionados cumplan las condiciones de servicio, requisitos de licencia y leyes aplicables.

---

## 💡 Comentarios y sugerencias

Tech Card Manager continuará desarrollándose en torno a:

* Indexación Technical Specifications de solo lectura
* Tarjeta Emby Technical Specifications
* Presentación de la biblioteca multimedia
* Integración Web UI
* Experiencia de usuario de Windows
* Compatibilidad con Emby
* Estabilidad
* Flujos de desarrollo con Coding Agents

Si tienes ideas sobre el diseño de tarjetas, compatibilidad de medios, integración en páginas Emby, navegación del índice, uso en Windows o flujos de desarrollo, puedes participar mediante Issues.
