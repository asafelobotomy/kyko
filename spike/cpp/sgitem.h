#ifndef KYKO_SPIKE_SGITEM_H
#define KYKO_SPIKE_SGITEM_H

#include <QtCore/QHash>
#include <QtGui/QFont>
#include <QtGui/QImage>
#include <QtQuick/QQuickItem>

#include "rust/cxx.h"

struct FfiCell;
class QSGTexture;
class QQuickWindow;

// Renders the grid with scene-graph nodes. Mode "atlas": own glyph atlas, one textured quad per
// cell. Mode "text": one QSGTextNode per row. Measurement only: no keyboard input.
class SgItem : public QQuickItem {
    Q_OBJECT
    Q_PROPERTY(int cols READ cols NOTIFY gridChanged)
    Q_PROPERTY(int rows READ rows NOTIFY gridChanged)

public:
    explicit SgItem(QQuickItem* parent = nullptr);
    int cols() const { return m_cols; }
    int rows() const { return m_rows; }

Q_SIGNALS:
    void gridChanged();

protected:
    QSGNode* updatePaintNode(QSGNode* old, UpdatePaintNodeData*) override;
    void geometryChange(const QRectF& newGeometry, const QRectF& oldGeometry) override;
    void itemChange(ItemChange change, const ItemChangeData& data) override;

private:
    struct Slot {
        float u0, v0, u1, v1;
        bool wide;
    };
    Slot slotFor(quint32 ch, quint8 style);

    bool m_textMode = false;
    bool m_full = false;
    QFont m_font;
    qreal m_cellW = 8, m_cellH = 16, m_ascent = 12;
    int m_cols = 0, m_rows = 0;
    bool m_started = false;

    QImage m_atlas;
    QHash<quint64, Slot> m_slots;
    int m_nextSlot = 0;
    bool m_atlasDirty = true;
    QSGTexture* m_texture = nullptr;

    QList<quint64> m_rowHash;
    std::vector<double> m_buildMs;
};

// Attach whole-frame timing to a window (once). Prints every 300 frames when KYKO_SPIKE_STATS is set.
void frame_stats_attach(QQuickWindow* window);

#endif
