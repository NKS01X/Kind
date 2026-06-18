package client

import (
	"context"
	pb "vortex/client/proto"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

type KindClient struct {
	conn   *grpc.ClientConn
	client pb.KindServiceClient
}

// NewKindClient creates a new gRPC client for Kind DB.
func NewKindClient(addr string) (*KindClient, error) {
	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, err
	}
	client := pb.NewKindServiceClient(conn)
	return &KindClient{
		conn:   conn,
		client: client,
	}, nil
}

// Close closes the connection to the gRPC server.
func (c *KindClient) Close() error {
	return c.conn.Close()
}

// Get retrieves a record by key.
func (c *KindClient) Get(ctx context.Context, key string) (*pb.Record, error) {
	req := &pb.GetRequest{Key: key}
	return c.client.Get(ctx, req)
}

// Put inserts or updates a record.
func (c *KindClient) Put(ctx context.Context, key string, value []byte) (bool, error) {
	req := &pb.PutRequest{
		Key:   key,
		Value: value,
	}
	res, err := c.client.Put(ctx, req)
	if err != nil {
		return false, err
	}
	return res.Success, nil
}

// Delete removes a record by key.
func (c *KindClient) Delete(ctx context.Context, key string) (bool, error) {
	req := &pb.DeleteRequest{Key: key}
	res, err := c.client.Delete(ctx, req)
	if err != nil {
		return false, err
	}
	return res.Success, nil
}

// RangeScan retrieves records between lo and hi (inclusive).
func (c *KindClient) RangeScan(ctx context.Context, lo string, hi string) ([]*pb.Record, error) {
	req := &pb.RangeScanRequest{
		Lo: lo,
		Hi: hi,
	}
	res, err := c.client.RangeScan(ctx, req)
	if err != nil {
		return nil, err
	}
	return res.Records, nil
}
