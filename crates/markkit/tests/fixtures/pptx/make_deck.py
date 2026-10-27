from pptx import Presentation
from pptx.util import Inches
import sys
p=Presentation()
p.core_properties.title="Quarterly Review"
s=p.slides.add_slide(p.slide_layouts[0]); s.shapes.title.text="Q3 Results"; s.placeholders[1].text="Prepared by Finance"
s=p.slides.add_slide(p.slide_layouts[1]); s.shapes.title.text="Highlights"
tf=s.placeholders[1].text_frame; tf.text="Revenue up 12%"
para=tf.add_paragraph(); para.text="EMEA strongest"; para.level=1
para=tf.add_paragraph(); r=para.add_run(); r.text="Bold "; r.font.bold=True; r2=para.add_run(); r2.text="docs"; r2.hyperlink.address="https://example.com/q3"
s.notes_slide.notes_text_frame.text="Mention the hiring freeze."
s=p.slides.add_slide(p.slide_layouts[5]); s.shapes.title.text="Numbers"
t=s.shapes.add_table(3,2,Inches(1),Inches(2),Inches(4),Inches(2)).table
for i,row in enumerate([["Region","Revenue"],["EMEA","1.2M"],["APAC","0.9 | 1.0M"]]):
    for j,v in enumerate(row): t.cell(i,j).text=v
tb=s.shapes.add_textbox(Inches(1),Inches(5),Inches(4),Inches(1)); tb.text_frame.text="Source: internal"
s=p.slides.add_slide(p.slide_layouts[1]); s.shapes.title.text="Secret"; s.placeholders[1].text="hidden content"
s._element.set("show","0")
p.save(sys.argv[1])
