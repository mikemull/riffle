import { Map as MaplibreMap } from "/public/vendor/maplibre-gl.mjs";

// Everything about the MapLibre instance's lifecycle stays in here. Rust
// only ever calls initMap/setSites/onSiteClick and receives a clicked
// site_no string back through the onSiteClick callback -- see
// components/site_map.rs.

const maps = {};
const pendingSites = {};

// A keyless raster basemap straight from OSM tiles -- no API key, no
// dependency on MapLibre's own demo-style server staying up.
const OSM_STYLE = {
  version: 8,
  sources: {
    osm: {
      type: "raster",
      tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
      tileSize: 256,
      attribution: "&copy; OpenStreetMap contributors",
    },
  },
  layers: [{ id: "osm", type: "raster", source: "osm" }],
};

export function initMap(containerId) {
  // The Rust side calls this as soon as its NodeRef resolves to Some, but
  // that doesn't guarantee the element is actually attached to the live
  // document yet (a real race we hit: MapLibre's constructor throws
  // "Container '...' not found", which -- crossing back over the
  // wasm_bindgen FFI boundary -- becomes an unrecoverable Rust panic and
  // traps the whole WASM module, taking the rest of the page down with it).
  // Retry on the next frame instead of trusting the caller's timing.
  if (!document.getElementById(containerId)) {
    requestAnimationFrame(() => initMap(containerId));
    return;
  }

  const map = new MaplibreMap({
    container: containerId,
    style: OSM_STYLE,
    center: [-84.3, 41.25], // roughly the Maumee/St Joseph/St Marys confluence
    zoom: 8,
  });

  map.on("load", () => {
    map.addSource("sites", {
      type: "geojson",
      data: { type: "FeatureCollection", features: [] },
    });
    map.addLayer({
      id: "sites-layer",
      type: "circle",
      source: "sites",
      paint: {
        "circle-radius": 8,
        "circle-color": "#2563eb",
        "circle-stroke-width": 2,
        "circle-stroke-color": "#ffffff",
      },
    });

    if (pendingSites[containerId]) {
      map.getSource("sites").setData(pendingSites[containerId]);
      delete pendingSites[containerId];
    }
  });

  maps[containerId] = map;
}

function toGeoJson(sitesJson) {
  const sites = JSON.parse(sitesJson);
  return {
    type: "FeatureCollection",
    features: sites.map((s) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [s.longitude, s.latitude] },
      properties: { site_no: s.site_no },
    })),
  };
}

export function setSites(containerId, sitesJson) {
  const geojson = toGeoJson(sitesJson);
  const map = maps[containerId];
  const source = map && map.getSource("sites");
  if (source) {
    source.setData(geojson);
  } else {
    // Map exists but hasn't fired "load" yet -- apply once it does.
    pendingSites[containerId] = geojson;
  }
}

export function onSiteClick(containerId, callback) {
  const map = maps[containerId];
  if (!map) {
    // initMap may still be waiting for its own retry (see above) --
    // without this, a call that arrives before the map exists would
    // silently register no click handler at all, forever.
    requestAnimationFrame(() => onSiteClick(containerId, callback));
    return;
  }

  const register = () => {
    map.on("click", "sites-layer", (e) => {
      callback(e.features[0].properties.site_no);
    });
    map.on("mouseenter", "sites-layer", () => {
      map.getCanvas().style.cursor = "pointer";
    });
    map.on("mouseleave", "sites-layer", () => {
      map.getCanvas().style.cursor = "";
    });
  };

  if (map.loaded()) {
    register();
  } else {
    map.on("load", register);
  }
}
