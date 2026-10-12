-- Raw support mail is deleted 90 days after it arrived (customer_relations::retention).
CREATE INDEX inbound_mail_received ON inbound_mail (received_at);
