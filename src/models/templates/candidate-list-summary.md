+++
title = "Overzicht kandidatenlijsten"
language = "nl"
footer_right = "Pagina {page} van {total}"
+++

## Overzicht van de ingeleverde kandidatenlijsten

# Centraal Stembureau

Voor de verkiezing van de leden van **{{ election_name|line }}** op **{{ election_date|line }}**

De voorzitter van het centraal stembureau voor de verkiezing van de leden van **{{ election_name | line}}**,

|  |  | Lijstengroep (gelijkluidende lijsten) | Lijstengroep (niet gelijkluidende lijsten) | Op zichzelf staande lijst |
{%- for district in electoral_districts -%} {{ district.region_number() }} |
{%- endfor %}
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
{% for (political_group, district_batches) in lists -%}
  | {{ loop.index }} | {{ political_group }} | X | X | X |
  {%- let active_districts = self.active_districts(&political_group) %}
    {%- for district in electoral_districts -%}
      {%- if active_districts.contains(&district) -%}
        X |
      {%- else -%}
        |
      {%- endif -%}
    {%- endfor %}
{% endfor %}


Kieskringen en gemeente of openbaar lichaam waar hoofdstembureau is gevestigd:
{% for district in electoral_districts %}
{{ district.region_number() }}. {{ district.title() }}
{%- endfor %}

{% for (political_group, district_batches) in lists %}
#### {{ loop.index }}. {{ political_group }}

{ widths = "1 8"}
| Stel | Kieskringen |
| --- | --- |
{% for (number, districts) in self.batched_districts(political_group) -%}
| {{ number | assigned_or("") }} | {{ districts }} |
{% endfor %}
{% endfor %}
