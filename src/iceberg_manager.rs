use super::data::DataGen;

use std::{collections::HashMap, future, sync::Arc};

use anyhow::bail;
use futures::stream::StreamExt;
use iceberg::{
    spec::{DataFileFormat, NestedField, PrimitiveType, Schema, TableMetadata, Type},
    table::Table,
    writer::file_writer::{
        location_generator::{DefaultFileNameGenerator, DefaultLocationGenerator},
        FileWriter, FileWriterBuilder, ParquetWriterBuilder,
    },
    Catalog, NamespaceIdent, TableCreation, TableIdent,
};
use iceberg_catalog_rest::{RestCatalog, RestCatalogConfig};
use parquet::file::properties::WriterProperties;

pub struct IcebergManager {
    catalog: RestCatalog,
    namespace_id: NamespaceIdent,
    schema: Schema,
}

impl IcebergManager {
    pub async fn new(
        catalog_uri: String,
        namespace_id: String,
        warehouse_name: String,
    ) -> anyhow::Result<Self> {
        let config = RestCatalogConfig::builder()
            .uri(catalog_uri)
            .warehouse(warehouse_name)
            .build();

        let catalog = RestCatalog::new(config);

        // init namespace
        let namespace_id = NamespaceIdent::new(namespace_id);

        if !catalog.namespace_exists(&namespace_id).await? {
            catalog
                .create_namespace(&namespace_id, HashMap::new())
                .await?;
        }

        let schema_builder = Schema::builder();

        let schema = schema_builder
            .with_fields(vec![
                Arc::new(NestedField {
                    id: 0,
                    name: "id".to_string(),
                    required: true,
                    field_type: Box::new(Type::Primitive(PrimitiveType::Int)),
                    doc: None,
                    initial_default: None,
                    write_default: None,
                }),
                Arc::new(NestedField {
                    id: 1,
                    name: "value".to_string(),
                    required: true,
                    field_type: Box::new(Type::Primitive(PrimitiveType::String)),
                    doc: None,
                    initial_default: None,
                    write_default: None,
                }),
            ])
            .build()?;

        // let schema = arrow_schema_to_schema(&datagen.arrow_schema)?; // This does not work

        println!("built schema");

        Ok(Self {
            catalog,
            namespace_id,
            schema,
        })
    }

    pub async fn create_table(&self, tbl_name: String) -> anyhow::Result<()> {
        let table_id = TableIdent::new(self.namespace_id.clone(), tbl_name.clone());

        let table_creation = TableCreation::builder()
            .name(table_id.name.clone())
            .schema(self.schema.clone())
            .build();

        let tbl = self
            .catalog
            .create_table(&table_id.namespace, table_creation)
            .await?;

        println!("Table created: {:?}", tbl.metadata());

        Ok(())
    }

    pub async fn load_table(&self, tbl_name: &str) -> anyhow::Result<Table> {
        let table_idents = self.catalog.list_tables(&self.namespace_id).await?;

        let tbl_ident_opt = table_idents.iter().find(|tbl_ident| {
            dbg!(tbl_ident);
            tbl_ident.name() == tbl_name
        });

        match tbl_ident_opt {
            Some(tbl_ident) => Ok(self.catalog.load_table(&tbl_ident).await?),

            None => bail!("table not found"),
        }
    }

    pub async fn write_data(self, tbl_name: &str, datagen: DataGen) -> anyhow::Result<()> {
        let tbl = self.load_table(tbl_name).await?;

        let tbl_metadata = tbl.metadata().clone();
        let file_io = tbl.file_io();

        //let mut props = HashMap::new();
        //
        //props.insert("s3.endpoint", "s3://warehouse/");
        //props.insert("s3.access-key-id", "admin");
        //props.insert("s3.secret-access-key", "password");
        //props.insert("s3.region", "local-01");
        //
        //let file_io = FileIOBuilder::new("s3").with_props(props).build()?;

        println!("File IO created");

        let loc_gen = DefaultLocationGenerator::new(tbl_metadata)?;
        let file_name_gen =
            DefaultFileNameGenerator::new(String::new(), None, DataFileFormat::Parquet);

        // register a writer
        let mut writer = ParquetWriterBuilder::new(
            WriterProperties::new(),
            Arc::new(self.schema.clone()),
            file_io.clone(),
            loc_gen,
            file_name_gen,
        )
        .build()
        .await?;

        println!("Created Parquet Writer");

        // write generated data to it
        let record_batch = datagen.convert_to_arrow_record_batch(datagen.gen_n_data(5));

        writer.write(&record_batch).await?;

        println!("Wrote data to it: {:?}", record_batch);

        Ok(())
    }

    pub async fn list_tables(&self) -> anyhow::Result<()> {
        let tables = self.catalog.list_tables(&self.namespace_id).await?;
        for table in tables {
            println!("{}", table.name);
        }

        Ok(())
    }

    pub async fn drop_table(&self, tbl_name: String) -> anyhow::Result<()> {
        let table_id = TableIdent::new(self.namespace_id.clone(), tbl_name);
        self.catalog.drop_table(&table_id).await?;

        Ok(())
    }

    pub async fn create_snapshot() -> anyhow::Result<i64> {
        //SnapshotBuilder::with_snapshot_id(0).with_sequence_numberbuild();
        Ok(0)
    }

    pub async fn list_snapshots(&self, tbl_metadata: TableMetadata) -> anyhow::Result<Vec<i64>> {
        let snapshots = tbl_metadata.snapshots();

        let mut snapshot_ids = vec![];
        for snapshot in snapshots {
            println!("Snapshot: {}", snapshot.snapshot_id());
            snapshot_ids.push(snapshot.snapshot_id());
        }

        Ok(snapshot_ids)
    }

    pub async fn read_table_at_version(self, tbl: Table, version: i64) -> anyhow::Result<()> {
        let record_batch_results = tbl
            .scan()
            .select_all()
            .snapshot_id(version)
            .build()?
            .to_arrow()
            .await?;

        record_batch_results
            .for_each(|result| {
                if let Ok(record_batch) = result {
                    println!("Record Batch: {:?}", record_batch);
                }

                future::ready(())
            })
            .await;

        Ok(())
    }
}

/*
* -> create database catalog_database;
* -> ./bin/iceberg-catalog migrate
* -> ./bin/iceberg-catalog serve
* -> curl -X POST http://localhost:8060/management/v1/warehouse -H "Content-Type: application/json" -d @create-warehouse-request.json
* -> paste:
* ```
spark.jars.packages                                  org.apache.iceberg:iceberg-spark-runtime-3.5_2.12:1.6.1
spark.sql.extensions                                 org.apache.iceberg.spark.extensions.IcebergSparkSessionExtensions

spark.sql.defaultCatalog                             local
spark.sql.catalog.local                              org.apache.iceberg.spark.SparkCatalog
spark.sql.catalog.local.catalog-impl                 org.apache.iceberg.rest.RESTCatalog
spark.sql.catalog.local.uri                          http://iceberg-rest-catalog-rs:8060/catalog
spark.sql.catalog.local.token                        dummy
spark.sql.catalog.local.warehouse                    00000000-0000-0000-0000-000000000000/test

spark.sql.catalog.spark_catalog                      org.apache.iceberg.spark.SparkSessionCatalog
spark.sql.catalog.spark_catalog.type                 hive
* ```
* -> create namespace ns_1;
* -> create table ns_1.tbl(id bigint, value string) using iceberg;
* -> insert into ns_1.tbl values(1, "1");
*/
