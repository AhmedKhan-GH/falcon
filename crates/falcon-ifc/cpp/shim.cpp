// A small C interface to web-ifc for falcon: open an IFC file from memory and
// hand every product's placed geometry to a callback, the way Online 3D Viewer
// reads it (LoadAllGeometry with COORDINATE_TO_ORIGIN, openings and spaces
// left out). No C++ exception leaves this file.
//
// This file is falcon's own (MIT OR Apache-2.0); it only calls web-ifc's
// public classes.

#include <algorithm>
#include <cstdint>
#include <cstring>
#include <exception>
#include <string>
#include <vector>

#include <spdlog/spdlog.h>

#include "web-ifc/modelmanager/ModelManager.h"
#include "web-ifc/schema/IfcSchemaManager.h"
#include "web-ifc/schema/ifc-schema.h"

extern "C" {

// One placed geometry of a product. `vertices` holds six floats per vertex
// (position, then normal) in the geometry's own frame; `matrix` is the
// column-major 4x4 placement; `color` is RGBA in 0..1. Returns non-zero to
// stop reading.
typedef int (*falcon_ifc_geometry_fn)(void *ctx, uint32_t product, const float *vertices, uint32_t vertex_floats,
                                      const uint32_t *indices, uint32_t index_count, const double *color,
                                      const double *matrix);

// One product looked at, with its entity name (upper case, e.g. IFCWALL) and
// how many geometries it produced.
typedef void (*falcon_ifc_product_fn)(void *ctx, uint32_t product, const char *type_name, uint32_t geometries);

// Reads `len` bytes of IFC text. Returns 0 when the file was read (with or
// without geometry), 1 when web-ifc could not open it, 2 when it failed
// with an error (message in `err`), 3 when the callback stopped it.
int falcon_ifc_read(const uint8_t *data, size_t len, uint16_t circle_segments, void *ctx,
                    falcon_ifc_product_fn on_product, falcon_ifc_geometry_fn on_geometry, char *err, size_t err_len)
{
    auto fail = [&](const char *msg) {
        if (err && err_len > 0) {
            std::strncpy(err, msg, err_len - 1);
            err[err_len - 1] = 0;
        }
    };
    try {
        spdlog::set_level(spdlog::level::off);
        webifc::manager::ModelManager manager(false);
        manager.SetLogLevel(spdlog::level::off);
        webifc::manager::LoaderSettings settings;
        settings.COORDINATE_TO_ORIGIN = true;
        if (circle_segments > 0) {
            settings.CIRCLE_SEGMENTS = circle_segments;
        }
        uint32_t model = manager.CreateModel(settings);
        auto loader = manager.GetIfcLoader(model);
        const std::function<uint32_t(char *, size_t, size_t)> feed = [&](char *dest, size_t offset, size_t size) -> uint32_t {
            if (offset >= len) {
                return 0;
            }
            size_t n = std::min(size, len - offset);
            std::memcpy(dest, data + offset, n);
            return (uint32_t)n;
        };
        if (!loader->LoadFile(feed)) {
            manager.CloseModel(model);
            return 1;
        }
        auto processor = manager.GetGeometryProcessor(model);
        const auto &schema = manager.GetSchemaManager();
        std::vector<uint32_t> types(schema.GetIfcElementList().begin(), schema.GetIfcElementList().end());
        std::sort(types.begin(), types.end());
        for (uint32_t type : types) {
            if (type == webifc::schema::IFCOPENINGELEMENT || type == webifc::schema::IFCSPACE ||
                type == webifc::schema::IFCOPENINGSTANDARDCASE) {
                continue;
            }
            std::string name;
            for (uint32_t id : loader->GetExpressIDsWithType(type)) {
                if (name.empty()) {
                    name = schema.IfcTypeCodeToType(type);
                }
                uint32_t produced = 0;
                bool stop = false;
                // One product that web-ifc cannot process is left out; the rest are read.
                try {
                    auto mesh = processor->GetFlatMesh(id, true);
                    for (auto &placed : mesh.geometries) {
                        auto &geometry = processor->GetGeometry(placed.geometryExpressID);
                        auto vertices = reinterpret_cast<const float *>(geometry.GetVertexData());
                        uint32_t vertex_floats = geometry.GetVertexDataSize();
                        // The index pointer is only taken from a non-empty list.
                        uint32_t index_count = geometry.GetIndexDataSize();
                        if (vertex_floats < 6 || index_count < 3 || !vertices) {
                            continue;
                        }
                        auto indices = reinterpret_cast<const uint32_t *>(geometry.GetIndexData());
                        double color[4] = {placed.color.r, placed.color.g, placed.color.b, placed.color.a};
                        produced++;
                        if (on_geometry(ctx, id, vertices, vertex_floats, indices, index_count, color,
                                        placed.flatTransformation.data()) != 0) {
                            stop = true;
                            break;
                        }
                    }
                } catch (...) {
                }
                on_product(ctx, id, name.c_str(), produced);
                processor->Clear(true);
                if (stop) {
                    manager.CloseModel(model);
                    return 3;
                }
            }
        }
        manager.CloseModel(model);
        return 0;
    } catch (const std::exception &e) {
        fail(e.what());
        return 2;
    } catch (...) {
        fail("unknown error");
        return 2;
    }
}

} // extern "C"
