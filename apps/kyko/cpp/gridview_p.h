// Internals the grid item's translation units share: cell flags, the glyph material, the
// background geometry and the drawn shapes. Not part of the item's interface (gridview.h).
#ifndef KYKO_GRIDVIEW_P_H
#define KYKO_GRIDVIEW_P_H

#include "gridview.h"

#include <QtCore/QPointer>
#include <QtGui/QPainter>
#include <QtQuick/QSGGeometryNode>
#include <QtQuick/QSGMaterial>

#include "kyko/src/bridge.cxx.h"

const quint8 BOLD = 1, ITALIC = 2, UNDERLINE = 4, WIDE = 16, WIDE_SPACER = 32;
const int ATLAS = 2048;
const quint8 UL_SINGLE = 1, UL_DOUBLE = 2, UL_CURLY = 3, UL_DOTTED = 4, UL_DASHED = 5;
const quint8 STRIKE = 64, DIM = 128;

// Every grid item, for the calls from Rust that reach them all; and the one controller.
extern QList<QPointer<GridView>> g_views;
extern QPointer<Controller> g_ctl;

inline QString str(const rust::String& s) { return QString::fromUtf8(s.data(), int(s.size())); }

// A colour faded towards another, for dim (SGR 2) text.
inline quint32 mix(quint32 a, quint32 b, int pctA) {
    auto ch = [&](int s) { return ((((a >> s) & 0xff) * pctA + ((b >> s) & 0xff) * (100 - pctA)) / 100) & 0xff; };
    return (ch(16) << 16) | (ch(8) << 8) | ch(0);
}

// ---- glyph material: alpha from the atlas, colour per vertex ------------------------------------
struct GlyphVertex {
    float x, y, u, v;
    uchar r, g, b, a;
};

class GlyphMaterial : public QSGMaterial {
public:
    GlyphMaterial() { setFlag(Blending, true); }
    QSGMaterialType* type() const override {
        static QSGMaterialType t;
        return &t;
    }
    QSGMaterialShader* createShader(QSGRendererInterface::RenderMode) const override;
    int compare(const QSGMaterial* o) const override {
        return texture == static_cast<const GlyphMaterial*>(o)->texture ? 0 : 1;
    }
    QSGTexture* texture = nullptr;
};

const QSGGeometry::AttributeSet& glyphAttributes();

inline void quad(QSGGeometry::ColoredPoint2D*& p, float x0, float y0, float x1, float y1, quint32 c) {
    const uchar r = (c >> 16) & 0xff, g = (c >> 8) & 0xff, b = c & 0xff;
    p[0].set(x0, y0, r, g, b, 255);
    p[1].set(x1, y0, r, g, b, 255);
    p[2].set(x0, y1, r, g, b, 255);
    p[3].set(x1, y0, r, g, b, 255);
    p[4].set(x1, y1, r, g, b, 255);
    p[5].set(x0, y1, r, g, b, 255);
    p += 6;
}

QSGGeometryNode* makeBgNode();

// Backgrounds: one quad for the whole item, one per run of cells with a non-default background,
// and the cursor block on top.
// The gutter: a thin bar at the left edge of the rows that belong to a command the shell marked,
// green when it succeeded, red when it failed, yellow while it runs.
constexpr float GUTTER_W = 3.0f;
void fillBg(QSGGeometryNode* node, const FfiCell* cells, const uint8_t* gutter, int cols, int rows, qreal cw,
            qreal ch, qreal w, qreal h, int curX, int curY, quint32 curColor, uchar alpha);

// Box drawing, blocks and Powerline separators, drawn rather than taken from the font (boxdraw.cpp).
bool drawnShape(quint32 ch);
// Draws `ch` into the w x h rectangle at (x, y); false when it is not one of the drawn shapes.
bool drawShape(QPainter& p, quint32 ch, int x, int y, int w, int h, int light);

#endif
